#!/usr/bin/env swift

import Foundation
#if canImport(Darwin)
import Darwin
#elseif canImport(Glibc)
import Glibc
#endif

private enum VaultCtlError: LocalizedError {
    case usage(String)
    case invalidArgument(String)
    case io(String)
    case runtime(String)

    var errorDescription: String? {
        switch self {
        case let .usage(message):
            return message
        case let .invalidArgument(message):
            return message
        case let .io(message):
            return message
        case let .runtime(message):
            return message
        }
    }
}

private enum SyncCommand: String, CaseIterable {
    case fullPush = "full-push"
    case fullPull = "full-pull"
    case deltaPush = "delta-push"
    case deltaPull = "delta-pull"
}

private struct StoredProfile: Codable {
    var baseURL: String?
    var apiKey: String?
    var vaultUID: String?
    var localPath: String?
    var deviceID: String?
    var limit: Int?
    var maxUploadBytes: Int?
    var stateFile: String?

    var isEmpty: Bool {
        baseURL == nil &&
        apiKey == nil &&
        vaultUID == nil &&
        localPath == nil &&
        deviceID == nil &&
        limit == nil &&
        maxUploadBytes == nil &&
        stateFile == nil
    }
}

private struct VaultCtlConfig: Codable {
    var defaultProfile: String?
    var profiles: [String: StoredProfile]

    static let empty = VaultCtlConfig(defaultProfile: nil, profiles: [:])
}

private struct ProfileOverrides {
    var baseURL: String?
    var apiKey: String?
    var vaultUID: String?
    var localPath: String?
    var deviceID: String?
    var limit: Int?
    var maxUploadBytes: Int?
    var stateFile: String?
    var selectedProfile: String?
    var setDefault: Bool = false
}

private enum TopLevelCommand {
    case help
    case configPath
    case profileList
    case profileShow(name: String?)
    case profileSave(name: String, overrides: ProfileOverrides)
    case profileUse(name: String)
    case profileRemove(name: String)
    case sync(command: SyncCommand, overrides: ProfileOverrides)
}

private let fileManager = FileManager.default

private func mainExitCode() -> Int32 {
    do {
        if shouldStartInteractiveShell(arguments: CommandLine.arguments) {
            try runInteractiveShell()
            return 0
        }

        let command = try parseCommand(arguments: CommandLine.arguments)
        return try run(command, interactive: false)
    } catch {
        FileHandle.standardError.write(Data("[vaultctl] ERROR: \(error.localizedDescription)\n".utf8))
        return 1
    }
}

private func run(_ command: TopLevelCommand, interactive: Bool) throws -> Int32 {
    switch command {
    case .help:
        print(usageText())
        return 0

    case .configPath:
        print(configFileURL().path)
        return 0

    case .profileList:
        let config = try loadConfig()
        printProfileList(config)
        return 0

    case let .profileShow(name):
        let config = try loadConfig()
        let resolvedName = try resolveProfileName(input: name, config: config)
        let profile = try requireProfile(named: resolvedName, config: config)
        printProfile(name: resolvedName, profile: profile, isDefault: config.defaultProfile == resolvedName)
        return 0

    case let .profileSave(name, overrides):
        try saveProfile(name: name, overrides: overrides)
        return 0

    case let .profileUse(name):
        try useProfile(name: name)
        return 0

    case let .profileRemove(name):
        try removeProfile(name: name)
        return 0

    case let .sync(command, overrides):
        return try runSync(command: command, overrides: overrides, interactive: interactive)
    }
}

private func shouldStartInteractiveShell(arguments: [String]) -> Bool {
    guard arguments.count > 1 else {
        return true
    }

    switch arguments[1] {
    case "shell", "repl", "interactive":
        return true
    default:
        return false
    }
}

private func runInteractiveShell() throws {
    print("VaultCtl interactive shell")
    print("Type `help` for commands. Type `quit` or `exit` to leave.")

    while true {
        print("vaultctl> ", terminator: "")
        fflush(stdout)

        guard let rawLine = readLine() else {
            print("")
            break
        }

        guard let line = trimmedNonEmpty(rawLine) else {
            continue
        }

        if line.hasPrefix("#") {
            continue
        }

        let lowered = line.lowercased()
        if lowered == "quit" || lowered == "exit" {
            break
        }

        do {
            let tokens = try splitCommandLine(line)
            if tokens.isEmpty {
                continue
            }

            let command = try parseCommand(arguments: ["vaultctl"] + tokens)
            let exitCode = try run(command, interactive: true)
            if exitCode != 0 {
                FileHandle.standardError.write(Data("[vaultctl] Command exited with status \(exitCode)\n".utf8))
            }
        } catch {
            FileHandle.standardError.write(Data("[vaultctl] ERROR: \(error.localizedDescription)\n".utf8))
        }
    }
}

private func parseCommand(arguments: [String]) throws -> TopLevelCommand {
    guard arguments.count > 1 else {
        return .help
    }

    let command = arguments[1]
    let rest = Array(arguments.dropFirst(2))

    if command == "--help" || command == "-h" || command == "help" {
        return .help
    }

    if command == "config" {
        guard let subcommand = rest.first else {
            throw VaultCtlError.usage("Missing `config` subcommand.\n\n\(usageText())")
        }
        if subcommand == "path" {
            return .configPath
        }
        throw VaultCtlError.invalidArgument("Unknown config subcommand: \(subcommand)")
    }

    if command == "profile" {
        return try parseProfileCommand(arguments: rest)
    }

    if command == "sync" {
        return try parseSyncCommand(arguments: rest)
    }

    if let syncCommand = SyncCommand(rawValue: command) {
        let overrides = try parseOverrides(arguments: rest)
        return .sync(command: syncCommand, overrides: overrides)
    }

    throw VaultCtlError.usage("Unknown command: \(command)\n\n\(usageText())")
}

private func parseProfileCommand(arguments: [String]) throws -> TopLevelCommand {
    guard let subcommand = arguments.first else {
        throw VaultCtlError.usage("Missing `profile` subcommand.\n\n\(usageText())")
    }

    let rest = Array(arguments.dropFirst())
    switch subcommand {
    case "list":
        return .profileList
    case "show":
        return .profileShow(name: rest.first)
    case "save":
        guard let name = rest.first else {
            throw VaultCtlError.invalidArgument("Missing profile name for `profile save`")
        }
        let overrides = try parseOverrides(arguments: Array(rest.dropFirst()))
        return .profileSave(name: name, overrides: overrides)
    case "use":
        guard let name = rest.first else {
            throw VaultCtlError.invalidArgument("Missing profile name for `profile use`")
        }
        return .profileUse(name: name)
    case "remove", "delete":
        guard let name = rest.first else {
            throw VaultCtlError.invalidArgument("Missing profile name for `profile remove`")
        }
        return .profileRemove(name: name)
    default:
        throw VaultCtlError.invalidArgument("Unknown profile subcommand: \(subcommand)")
    }
}

private func parseSyncCommand(arguments: [String]) throws -> TopLevelCommand {
    guard let raw = arguments.first else {
        throw VaultCtlError.invalidArgument("Missing sync command")
    }
    guard let command = SyncCommand(rawValue: raw) else {
        throw VaultCtlError.invalidArgument("Unknown sync command: \(raw)")
    }
    let overrides = try parseOverrides(arguments: Array(arguments.dropFirst()))
    return .sync(command: command, overrides: overrides)
}

private func parseOverrides(arguments: [String]) throws -> ProfileOverrides {
    var overrides = ProfileOverrides()
    var index = 0

    while index < arguments.count {
        let arg = arguments[index]
        index += 1

        switch arg {
        case "--base-url":
            overrides.baseURL = try nextValue(flag: arg, index: &index, arguments: arguments)
        case "--api-key":
            overrides.apiKey = try nextValue(flag: arg, index: &index, arguments: arguments)
        case "--vault-uid":
            overrides.vaultUID = try nextValue(flag: arg, index: &index, arguments: arguments)
        case "--local-path":
            overrides.localPath = normalizePathString(try nextValue(flag: arg, index: &index, arguments: arguments))
        case "--device-id":
            overrides.deviceID = try nextValue(flag: arg, index: &index, arguments: arguments)
        case "--limit":
            let raw = try nextValue(flag: arg, index: &index, arguments: arguments)
            guard let value = Int(raw), value > 0 else {
                throw VaultCtlError.invalidArgument("--limit must be a positive integer")
            }
            overrides.limit = value
        case "--max-upload-bytes":
            let raw = try nextValue(flag: arg, index: &index, arguments: arguments)
            guard let value = Int(raw), value > 0 else {
                throw VaultCtlError.invalidArgument("--max-upload-bytes must be a positive integer")
            }
            overrides.maxUploadBytes = value
        case "--state-file":
            overrides.stateFile = normalizePathString(try nextValue(flag: arg, index: &index, arguments: arguments))
        case "--profile":
            overrides.selectedProfile = try nextValue(flag: arg, index: &index, arguments: arguments)
        case "--set-default":
            overrides.setDefault = true
        case "--help", "-h":
            throw VaultCtlError.usage(usageText())
        default:
            throw VaultCtlError.invalidArgument("Unknown option: \(arg)")
        }
    }

    return overrides
}

private func saveProfile(name: String, overrides: ProfileOverrides) throws {
    let normalizedName = try normalizeProfileName(name)
    var config = try loadConfig()
    var profile = config.profiles[normalizedName] ?? StoredProfile()

    if let baseURL = trimmedNonEmpty(overrides.baseURL) {
        profile.baseURL = baseURL
    }
    if let apiKey = trimmedNonEmpty(overrides.apiKey) {
        profile.apiKey = apiKey
    }
    if let vaultUID = trimmedNonEmpty(overrides.vaultUID) {
        profile.vaultUID = vaultUID
    }
    if let localPath = trimmedNonEmpty(overrides.localPath) {
        profile.localPath = localPath
    }
    if let deviceID = trimmedNonEmpty(overrides.deviceID) {
        profile.deviceID = deviceID
    }
    if let limit = overrides.limit {
        profile.limit = limit
    }
    if let maxUploadBytes = overrides.maxUploadBytes {
        profile.maxUploadBytes = maxUploadBytes
    }
    if let stateFile = trimmedNonEmpty(overrides.stateFile) {
        profile.stateFile = stateFile
    }

    guard !profile.isEmpty else {
        throw VaultCtlError.invalidArgument("`profile save` needs at least one value to store")
    }

    config.profiles[normalizedName] = profile
    if config.defaultProfile == nil || overrides.setDefault {
        config.defaultProfile = normalizedName
    }

    try saveConfig(config)
    print("Saved profile `\(normalizedName)`.")
}

private func useProfile(name: String) throws {
    let normalizedName = try normalizeProfileName(name)
    var config = try loadConfig()
    _ = try requireProfile(named: normalizedName, config: config)
    config.defaultProfile = normalizedName
    try saveConfig(config)
    print("Default profile set to `\(normalizedName)`.")
}

private func removeProfile(name: String) throws {
    let normalizedName = try normalizeProfileName(name)
    var config = try loadConfig()
    guard config.profiles.removeValue(forKey: normalizedName) != nil else {
        throw VaultCtlError.invalidArgument("Profile not found: \(normalizedName)")
    }
    if config.defaultProfile == normalizedName {
        config.defaultProfile = config.profiles.keys.sorted().first
    }
    try saveConfig(config)
    print("Removed profile `\(normalizedName)`.")
}

private func runSync(command: SyncCommand, overrides: ProfileOverrides, interactive _: Bool) throws -> Int32 {
    let config = try loadConfig()
    let selectedProfileName = overrides.selectedProfile ?? config.defaultProfile
    let storedProfile = selectedProfileName.flatMap { config.profiles[$0] }

    if let selectedProfileName, storedProfile == nil {
        throw VaultCtlError.invalidArgument("Profile not found: \(selectedProfileName)")
    }

    let resolved = mergeProfile(storedProfile, overrides: overrides)
    guard let localPath = trimmedNonEmpty(resolved.localPath) else {
        throw VaultCtlError.invalidArgument("Missing local path. Set it in a profile or pass --local-path.")
    }

    let syncScriptURL = syncScriptFileURL()
    guard fileManager.isExecutableFile(atPath: syncScriptURL.path) else {
        throw VaultCtlError.runtime("Sync engine is not executable: \(syncScriptURL.path)")
    }

    let process = Process()
    process.executableURL = syncScriptURL
    process.arguments = buildSyncArguments(command: command, profile: resolved, localPath: localPath)
    process.currentDirectoryURL = syncScriptURL.deletingLastPathComponent()
    process.environment = buildProcessEnvironment()
    process.standardOutput = FileHandle.standardOutput
    process.standardError = FileHandle.standardError

    if let selectedProfileName {
        print("Using profile `\(selectedProfileName)`.")
    }

    try process.run()
    process.waitUntilExit()

    if process.terminationReason != .exit {
        throw VaultCtlError.runtime("Sync process terminated unexpectedly")
    }
    return process.terminationStatus
}

private func buildSyncArguments(command: SyncCommand, profile: StoredProfile, localPath: String) -> [String] {
    var arguments = [command.rawValue, "--local-path", localPath]

    if let baseURL = trimmedNonEmpty(profile.baseURL) {
        arguments += ["--base-url", baseURL]
    }
    if let apiKey = trimmedNonEmpty(profile.apiKey) {
        arguments += ["--api-key", apiKey]
    }
    if let vaultUID = trimmedNonEmpty(profile.vaultUID) {
        arguments += ["--vault-uid", vaultUID]
    }
    if let deviceID = trimmedNonEmpty(profile.deviceID) {
        arguments += ["--device-id", deviceID]
    }
    if let limit = profile.limit {
        arguments += ["--limit", String(limit)]
    }
    if let maxUploadBytes = profile.maxUploadBytes {
        arguments += ["--max-upload-bytes", String(maxUploadBytes)]
    }
    if let stateFile = trimmedNonEmpty(profile.stateFile) {
        arguments += ["--state-file", stateFile]
    }

    return arguments
}

private func buildProcessEnvironment() -> [String: String] {
    var environment = ProcessInfo.processInfo.environment
    if trimmedNonEmpty(environment["SWIFT_MODULECACHE_PATH"]) == nil {
        environment["SWIFT_MODULECACHE_PATH"] = "/tmp/swift-module-cache"
    }
    if trimmedNonEmpty(environment["CLANG_MODULE_CACHE_PATH"]) == nil {
        environment["CLANG_MODULE_CACHE_PATH"] = "/tmp/clang-module-cache"
    }
    return environment
}

private func mergeProfile(_ profile: StoredProfile?, overrides: ProfileOverrides) -> StoredProfile {
    var resolved = profile ?? StoredProfile()

    if let baseURL = overrides.baseURL {
        resolved.baseURL = baseURL
    }
    if let apiKey = overrides.apiKey {
        resolved.apiKey = apiKey
    }
    if let vaultUID = overrides.vaultUID {
        resolved.vaultUID = vaultUID
    }
    if let localPath = overrides.localPath {
        resolved.localPath = localPath
    }
    if let deviceID = overrides.deviceID {
        resolved.deviceID = deviceID
    }
    if let limit = overrides.limit {
        resolved.limit = limit
    }
    if let maxUploadBytes = overrides.maxUploadBytes {
        resolved.maxUploadBytes = maxUploadBytes
    }
    if let stateFile = overrides.stateFile {
        resolved.stateFile = stateFile
    }

    return resolved
}

private func printProfileList(_ config: VaultCtlConfig) {
    guard !config.profiles.isEmpty else {
        print("No saved profiles.")
        return
    }

    for name in config.profiles.keys.sorted() {
        let profile = config.profiles[name] ?? StoredProfile()
        let defaultMarker = config.defaultProfile == name ? "*" : " "
        let baseURL = profile.baseURL ?? "-"
        let localPath = profile.localPath ?? "-"
        let vaultUID = profile.vaultUID ?? "-"
        print("\(defaultMarker) \(name)  url=\(baseURL)  vault=\(vaultUID)  path=\(localPath)")
    }
}

private func printProfile(name: String, profile: StoredProfile, isDefault: Bool) {
    print("name: \(name)")
    print("default: \(isDefault ? "yes" : "no")")
    print("base_url: \(profile.baseURL ?? "-")")
    print("api_key: \(maskSecret(profile.apiKey))")
    print("vault_uid: \(profile.vaultUID ?? "-")")
    print("local_path: \(profile.localPath ?? "-")")
    print("device_id: \(profile.deviceID ?? "-")")
    print("limit: \(profile.limit.map(String.init) ?? "-")")
    print("max_upload_bytes: \(profile.maxUploadBytes.map(String.init) ?? "-")")
    print("state_file: \(profile.stateFile ?? "-")")
}

private func resolveProfileName(input: String?, config: VaultCtlConfig) throws -> String {
    if let input {
        return try normalizeProfileName(input)
    }
    if let defaultProfile = config.defaultProfile {
        return defaultProfile
    }
    throw VaultCtlError.invalidArgument("No profile specified and no default profile is set")
}

private func requireProfile(named name: String, config: VaultCtlConfig) throws -> StoredProfile {
    guard let profile = config.profiles[name] else {
        throw VaultCtlError.invalidArgument("Profile not found: \(name)")
    }
    return profile
}

private func loadConfig() throws -> VaultCtlConfig {
    let url = configFileURL()
    guard fileManager.fileExists(atPath: url.path) else {
        return .empty
    }

    do {
        let data = try Data(contentsOf: url)
        return try JSONDecoder().decode(VaultCtlConfig.self, from: data)
    } catch {
        throw VaultCtlError.io("Failed to load config: \(error.localizedDescription)")
    }
}

private func saveConfig(_ config: VaultCtlConfig) throws {
    let url = configFileURL()
    do {
        try fileManager.createDirectory(at: url.deletingLastPathComponent(), withIntermediateDirectories: true)
        let encoder = JSONEncoder()
        encoder.outputFormatting = [.prettyPrinted, .sortedKeys]
        let data = try encoder.encode(config)
        try data.write(to: url, options: .atomic)
    } catch {
        throw VaultCtlError.io("Failed to save config: \(error.localizedDescription)")
    }
}

private func configFileURL() -> URL {
    if let override = trimmedNonEmpty(ProcessInfo.processInfo.environment["FOUNDATION_VAULTCTL_CONFIG"]) {
        return URL(fileURLWithPath: normalizePathString(override), isDirectory: false).standardizedFileURL
    }

    return fileManager.homeDirectoryForCurrentUser
        .appendingPathComponent(".foundation", isDirectory: true)
        .appendingPathComponent("vaultctl", isDirectory: true)
        .appendingPathComponent("config.json", isDirectory: false)
}

private func syncScriptFileURL() -> URL {
    URL(fileURLWithPath: #filePath)
        .deletingLastPathComponent()
        .appendingPathComponent("vault_sync.swift", isDirectory: false)
}

private func normalizeProfileName(_ raw: String) throws -> String {
    guard let trimmed = trimmedNonEmpty(raw) else {
        throw VaultCtlError.invalidArgument("Profile name cannot be empty")
    }
    if trimmed.contains("/") || trimmed.contains("\\") {
        throw VaultCtlError.invalidArgument("Profile name cannot contain path separators")
    }
    return trimmed
}

private func normalizePathString(_ raw: String) -> String {
    URL(fileURLWithPath: (raw as NSString).expandingTildeInPath).standardizedFileURL.path
}

private func trimmedNonEmpty(_ raw: String?) -> String? {
    guard let raw else {
        return nil
    }
    let trimmed = raw.trimmingCharacters(in: .whitespacesAndNewlines)
    return trimmed.isEmpty ? nil : trimmed
}

private func maskSecret(_ raw: String?) -> String {
    guard let raw = trimmedNonEmpty(raw) else {
        return "-"
    }
    if raw.count <= 8 {
        return "********"
    }
    return "\(raw.prefix(4))********\(raw.suffix(4))"
}

private func nextValue(flag: String, index: inout Int, arguments: [String]) throws -> String {
    guard index < arguments.count else {
        throw VaultCtlError.invalidArgument("Missing value for \(flag)")
    }
    let value = arguments[index]
    index += 1
    return value
}

private func splitCommandLine(_ line: String) throws -> [String] {
    enum Mode {
        case normal
        case singleQuote
        case doubleQuote
    }

    var tokens: [String] = []
    var current = ""
    var mode = Mode.normal
    var escaping = false

    for character in line {
        if escaping {
            current.append(character)
            escaping = false
            continue
        }

        switch mode {
        case .normal:
            if character == "\\" {
                escaping = true
            } else if character == "'" {
                mode = .singleQuote
            } else if character == "\"" {
                mode = .doubleQuote
            } else if character.isWhitespace {
                if !current.isEmpty {
                    tokens.append(current)
                    current.removeAll(keepingCapacity: true)
                }
            } else {
                current.append(character)
            }

        case .singleQuote:
            if character == "'" {
                mode = .normal
            } else {
                current.append(character)
            }

        case .doubleQuote:
            if character == "\\" {
                escaping = true
            } else if character == "\"" {
                mode = .normal
            } else {
                current.append(character)
            }
        }
    }

    if escaping {
        throw VaultCtlError.invalidArgument("Command ends with an unfinished escape sequence")
    }

    switch mode {
    case .normal:
        break
    case .singleQuote, .doubleQuote:
        throw VaultCtlError.invalidArgument("Command contains an unmatched quote")
    }

    if !current.isEmpty {
        tokens.append(current)
    }

    return tokens
}

private func usageText() -> String {
    """
    VaultCtl

    Usage:
      ./client/vaultctl.swift
      ./client/vaultctl.swift shell
      ./client/vaultctl.swift profile save <name> [options]
      ./client/vaultctl.swift profile list
      ./client/vaultctl.swift profile show [name]
      ./client/vaultctl.swift profile use <name>
      ./client/vaultctl.swift profile remove <name>
      ./client/vaultctl.swift config path
      ./client/vaultctl.swift sync <full-push|full-pull|delta-push|delta-pull> [options]
      ./client/vaultctl.swift <full-push|full-pull|delta-push|delta-pull> [options]

    Options:
      --profile <name>           Use a saved profile for sync
      --set-default              Make the saved profile the default
      --base-url <url>           Foundation server URL
      --api-key <key>            Foundation API key
      --vault-uid <uid>          Vault identifier
      --local-path <path>        Local vault path
      --device-id <id>           Device identifier
      --limit <n>                Pull/status limit
      --max-upload-bytes <n>     Per-request upload cap
      --state-file <path>        Override sync state file

    Storage:
      Default config file: ~/.foundation/vaultctl/config.json
      Override with env: FOUNDATION_VAULTCTL_CONFIG=/path/to/config.json

    Interactive shell:
      Run with no arguments to enter the shell loop
      Type `quit` or `exit` to leave

    Examples:
      ./client/vaultctl.swift profile save archive \\
        --base-url https://foundation.kyooni.kr \\
        --api-key host \\
        --vault-uid archive \\
        --local-path ~/Documents/archive \\
        --set-default

      ./client/vaultctl.swift profile list
      ./client/vaultctl.swift profile show archive
      ./client/vaultctl.swift full-push
      ./client/vaultctl.swift sync delta-pull --profile archive
      ./client/vaultctl.swift
      vaultctl> profile list
      vaultctl> delta-push
      vaultctl> quit
    """
}

exit(mainExitCode())
