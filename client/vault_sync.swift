#!/usr/bin/env swift

import Foundation
#if canImport(FoundationNetworking)
import FoundationNetworking
#endif
import Dispatch
#if canImport(Darwin)
import Darwin
#elseif canImport(Glibc)
import Glibc
#endif

private enum SyncScriptError: LocalizedError {
    case usage(String)
    case invalidArgument(String)
    case io(String)
    case network(String)
    case decoding(String)

    var errorDescription: String? {
        switch self {
        case let .usage(message):
            return message
        case let .invalidArgument(message):
            return message
        case let .io(message):
            return message
        case let .network(message):
            return message
        case let .decoding(message):
            return message
        }
    }
}

private enum VaultSyncCommand: String {
    case fullPush = "full-push"
    case fullPull = "full-pull"
    case deltaPush = "delta-push"
    case deltaPull = "delta-pull"
}

private struct CLIOptions {
    let command: VaultSyncCommand
    let baseURL: URL
    let apiKey: String
    let vaultUID: String
    let localPath: URL
    let deviceID: String
    let limit: Int?
    let maxUploadBytes: Int
    let stateFile: URL

    static func parse(arguments: [String]) throws -> CLIOptions? {
        let env = ProcessInfo.processInfo.environment
        var command: VaultSyncCommand?
        var baseURLRaw = env["FOUNDATION_BASE_URL"] ?? "http://localhost:8000"
        var apiKey: String? = env["FOUNDATION_API_KEY"]
        var vaultUID: String? = env["FOUNDATION_VAULT_UID"]
        var localPathRaw: String?
        var deviceID = ProcessInfo.processInfo.hostName
        var limit: Int?
        var maxUploadBytes = max(256 * 1024, parseInt(env["FOUNDATION_MAX_UPLOAD_BYTES"], defaultValue: 8 * 1024 * 1024))
        var stateFileRaw: String?

        var idx = 1
        while idx < arguments.count {
            let arg = arguments[idx]
            idx += 1

            switch arg {
            case "--help", "-h":
                printUsage()
                return nil
            case "--base-url":
                baseURLRaw = try nextValue(flag: arg, index: &idx, arguments: arguments)
            case "--api-key":
                apiKey = try nextValue(flag: arg, index: &idx, arguments: arguments)
            case "--vault-uid":
                vaultUID = try nextValue(flag: arg, index: &idx, arguments: arguments)
            case "--local-path":
                localPathRaw = try nextValue(flag: arg, index: &idx, arguments: arguments)
            case "--device-id":
                deviceID = try nextValue(flag: arg, index: &idx, arguments: arguments)
            case "--limit":
                let raw = try nextValue(flag: arg, index: &idx, arguments: arguments)
                guard let parsed = Int(raw), parsed > 0 else {
                    throw SyncScriptError.invalidArgument("--limit must be a positive integer")
                }
                limit = parsed
            case "--max-upload-bytes":
                let raw = try nextValue(flag: arg, index: &idx, arguments: arguments)
                guard let parsed = Int(raw), parsed > 0 else {
                    throw SyncScriptError.invalidArgument("--max-upload-bytes must be a positive integer")
                }
                maxUploadBytes = max(256 * 1024, parsed)
            case "--state-file":
                stateFileRaw = try nextValue(flag: arg, index: &idx, arguments: arguments)
            default:
                if arg.hasPrefix("--") {
                    throw SyncScriptError.invalidArgument("Unknown option: \(arg)")
                }
                guard command == nil else {
                    throw SyncScriptError.invalidArgument("Command provided more than once")
                }
                guard let parsedCommand = VaultSyncCommand(rawValue: arg) else {
                    throw SyncScriptError.invalidArgument("Unknown command: \(arg)")
                }
                command = parsedCommand
            }
        }

        guard let localPathRaw else {
            throw SyncScriptError.invalidArgument("Missing --local-path")
        }
        let resolvedCommand = command ?? .deltaPush
        guard let baseURL = URL(string: baseURLRaw) else {
            throw SyncScriptError.invalidArgument("Invalid --base-url: \(baseURLRaw)")
        }

        let localPath = URL(fileURLWithPath: localPathRaw, isDirectory: true).standardizedFileURL
        let derivedVaultUID = trimmedNonEmpty(vaultUID) ?? localPath.lastPathComponent
        guard let resolvedVaultUID = trimmedNonEmpty(derivedVaultUID) else {
            throw SyncScriptError.invalidArgument("Missing --vault-uid (and could not derive from --local-path)")
        }
        if trimmedNonEmpty(vaultUID) == nil {
            info("Using derived vault UID: \(resolvedVaultUID) (set --vault-uid or FOUNDATION_VAULT_UID to override).")
        }

        let resolvedAPIKey = trimmedNonEmpty(apiKey) ?? "host"
        if trimmedNonEmpty(apiKey) == nil {
            info("Using default API key: host (set --api-key or FOUNDATION_API_KEY to override).")
        }

        let stateFile: URL
        if let stateFileRaw {
            stateFile = URL(fileURLWithPath: stateFileRaw, isDirectory: false).standardizedFileURL
        } else {
            stateFile = localPath
                .appendingPathComponent(".foundation-sync", isDirectory: true)
                .appendingPathComponent("state.json", isDirectory: false)
        }

        return CLIOptions(
            command: resolvedCommand,
            baseURL: baseURL,
            apiKey: resolvedAPIKey,
            vaultUID: resolvedVaultUID,
            localPath: localPath,
            deviceID: deviceID.isEmpty ? "swift-sync-script" : deviceID,
            limit: limit,
            maxUploadBytes: maxUploadBytes,
            stateFile: stateFile
        )
    }
}

private protocol APIStatusResponse {
    var ok: Bool { get }
    var error: String? { get }
}

private struct DeltaChangePayload: Encodable {
    let file_path: String
    let action: String
    let changed_at_unix_ms: Int64?
    let content_base64: String?
    let content_sha256: String?
}

private struct DeltaPushPayload: Encodable {
    let vault_uid: String
    let device_id: String?
    let changes: [DeltaChangePayload]
}

private struct DeltaPullPayload: Encodable {
    let vault_uid: String
    let since_unix_ms: Int64?
    let limit: Int?
}

private struct StatusPayload: Encodable {
    let vault_uid: String
    let since_unix_ms: Int64?
    let limit: Int?
}

private struct FullPushFilePayload: Encodable {
    let file_path: String
    let content_base64: String
}

private struct FullPushPayload: Encodable {
    let vault_uid: String
    let device_id: String?
    let uploaded_at_unix_ms: Int64?
    let files: [FullPushFilePayload]
}

private struct FullPullPayload: Encodable {
    let vault_uid: String
    let limit: Int?
}

private struct PushResponse: Decodable, APIStatusResponse {
    let ok: Bool
    let vault_uid: String
    let applied_changes: Int
    let latest_change_id: Int64?
    let latest_change_unix_ms: Int64?
    let error: String?
}

private struct SnapshotFileItem: Decodable {
    let file_path: String
    let content_base64: String
    let content_sha256: String
    let size_bytes: Int64
    let updated_unix_ms: Int64
}

private struct ChangedFileItem: Decodable {
    let file_path: String
    let action: String
    let changed_at_unix_ms: Int64
    let content_base64: String?
    let content_sha256: String?
    let size_bytes: Int64?
}

private struct PullResponse: Decodable, APIStatusResponse {
    let ok: Bool
    let vault_uid: String
    let mode: String
    let since_unix_ms: Int64?
    let latest_change_id: Int64?
    let latest_change_unix_ms: Int64?
    let snapshot_files: [SnapshotFileItem]?
    let changed_files: [ChangedFileItem]?
    let error: String?
}

private struct StatusChangeLogItem: Decodable {
    let change_id: Int64
    let file_path: String
    let action: String
    let changed_at_unix_ms: Int64
    let device_id: String?
}

private struct StatusFileTimestampItem: Decodable {
    let file_path: String
    let updated_unix_ms: Int64
    let size_bytes: Int64
    let is_deleted: Bool
    let last_change_id: Int64?
}

private struct StatusResponse: Decodable, APIStatusResponse {
    let ok: Bool
    let vault_uid: String
    let since_unix_ms: Int64?
    let latest_change_id: Int64?
    let latest_change_unix_ms: Int64?
    let file_timestamps: [StatusFileTimestampItem]
    let change_log: [StatusChangeLogItem]
    let error: String?
}

private struct LocalFileMeta: Codable {
    let size_bytes: Int64
    let modified_unix_ms: Int64
}

private struct SyncState: Codable {
    let vault_uid: String
    var local_snapshot: [String: LocalFileMeta]
    var last_server_change_id: Int64?
    var last_server_change_unix_ms: Int64?
}

private final class VaultAPIClient {
    private let baseURL: URL
    private let apiKey: String
    private let session: URLSession
    private let encoder = JSONEncoder()
    private let decoder = JSONDecoder()

    init(baseURL: URL, apiKey: String, session: URLSession = .shared) {
        self.baseURL = baseURL
        self.apiKey = apiKey
        self.session = session
    }

    func post<Body: Encodable, Response: Decodable & APIStatusResponse>(
        path: String,
        body: Body
    ) async throws -> Response {
        guard let url = URL(string: path, relativeTo: baseURL) else {
            throw SyncScriptError.invalidArgument("Invalid URL path: \(path)")
        }

        var request = URLRequest(url: url)
        request.httpMethod = "POST"
        request.setValue("application/json", forHTTPHeaderField: "Content-Type")
        request.setValue("application/json", forHTTPHeaderField: "Accept")
        request.setValue("Bearer \(apiKey)", forHTTPHeaderField: "Authorization")
        request.httpBody = try encoder.encode(body)

        let (data, response) = try await session.data(for: request)
        guard let httpResponse = response as? HTTPURLResponse else {
            throw SyncScriptError.network("Invalid HTTP response")
        }

        guard (200..<300).contains(httpResponse.statusCode) else {
            let message = decodeServerMessage(data: data)
            throw SyncScriptError.network("HTTP \(httpResponse.statusCode): \(message)")
        }

        let decoded: Response
        do {
            decoded = try decoder.decode(Response.self, from: data)
        } catch {
            throw SyncScriptError.decoding("Failed to decode response for \(path): \(error.localizedDescription)")
        }

        if !decoded.ok {
            throw SyncScriptError.network(decoded.error ?? "Server returned ok=false for \(path)")
        }
        return decoded
    }

    private func decodeServerMessage(data: Data) -> String {
        if let json = try? JSONSerialization.jsonObject(with: data) as? [String: Any] {
            if let reason = json["reason"] as? String, !reason.isEmpty {
                return reason
            }
            if let error = json["error"] as? String, !error.isEmpty {
                return error
            }
        }

        if let text = String(data: data, encoding: .utf8), !text.isEmpty {
            return text
        }
        return "Unknown server error."
    }
}

private func runScript() async throws {
    guard let options = try CLIOptions.parse(arguments: CommandLine.arguments) else {
        return
    }

    try validateVaultUID(options.vaultUID)
    try ensureDirectoryExists(options.localPath)

    let api = VaultAPIClient(baseURL: options.baseURL, apiKey: options.apiKey)
    var state = loadState(stateFile: options.stateFile, vaultUID: options.vaultUID)

    switch options.command {
    case .fullPush:
        state = try await runFullPush(options: options, api: api, state: state)
    case .fullPull:
        state = try await runFullPull(options: options, api: api, state: state)
    case .deltaPush:
        state = try await runDeltaPush(options: options, api: api, state: state)
    case .deltaPull:
        state = try await runDeltaPull(options: options, api: api, state: state)
    }

    try saveState(state: state, stateFile: options.stateFile)
}

Task {
    do {
        try await runScript()
        exit(0)
    } catch {
        let message = error.localizedDescription
        FileHandle.standardError.write(Data("[vault-sync] ERROR: \(message)\n".utf8))
        exit(1)
    }
}

dispatchMain()

private func runFullPush(options: CLIOptions, api: VaultAPIClient, state: SyncState) async throws -> SyncState {
    let snapshot = try scanLocalVault(root: options.localPath)
    let payloadFiles = try snapshot.keys.sorted().map { path -> FullPushFilePayload in
        let data = try readLocalFile(root: options.localPath, relativePath: path)
        return FullPushFilePayload(file_path: path, content_base64: data.base64EncodedString())
    }

    let payload = FullPushPayload(
        vault_uid: options.vaultUID,
        device_id: options.deviceID,
        uploaded_at_unix_ms: unixMillisecondsNow(),
        files: payloadFiles
    )
    let fullPushPayloadBytes = try encodedByteCount(payload)

    if fullPushPayloadBytes <= options.maxUploadBytes {
        let response: PushResponse = try await api.post(path: "/vaults/sync/full-push", body: payload)

        info("Full push complete: \(payloadFiles.count) file(s), \(response.applied_changes) applied change(s).")

        var next = state
        next.local_snapshot = snapshot
        next.last_server_change_id = response.latest_change_id
        next.last_server_change_unix_ms = response.latest_change_unix_ms
        return next
    }

    warn(
        "full-push payload is \(fullPushPayloadBytes) bytes (max \(options.maxUploadBytes)); using batched delta push fallback."
    )

    let statusPayload = StatusPayload(vault_uid: options.vaultUID, since_unix_ms: nil, limit: options.limit)
    let status: StatusResponse = try await api.post(path: "/vaults/sync/status", body: statusPayload)
    let fallbackChanges = try buildDeltaChangesUsingRemoteStatus(
        localSnapshot: snapshot,
        remoteFileTimestamps: status.file_timestamps,
        localRoot: options.localPath,
        forceFullMirror: true
    )

    if fallbackChanges.isEmpty {
        info("Full push fallback found no differences; nothing to upload.")
        var next = state
        next.local_snapshot = snapshot
        next.last_server_change_id = status.latest_change_id
        next.last_server_change_unix_ms = status.latest_change_unix_ms
        return next
    }

    let pushResult = try await pushDeltaChangesInBatches(
        changes: fallbackChanges,
        options: options,
        api: api
    )
    info(
        "Full push fallback complete: sent \(pushResult.sentChanges) change(s) in \(pushResult.sentBatches) batch(es), applied \(pushResult.appliedChanges) change(s)."
    )

    var next = state
    next.local_snapshot = snapshot
    next.last_server_change_id = pushResult.latestChangeID
    next.last_server_change_unix_ms = pushResult.latestChangeUnixMS
    return next
}

private func runFullPull(options: CLIOptions, api: VaultAPIClient, state: SyncState) async throws -> SyncState {
    let payload = FullPullPayload(vault_uid: options.vaultUID, limit: options.limit)
    let response: PullResponse = try await api.post(path: "/vaults/sync/full-pull", body: payload)
    let snapshot = response.snapshot_files ?? []

    try applyFullSnapshot(snapshot, to: options.localPath)
    let localSnapshot = try scanLocalVault(root: options.localPath)

    info("Full pull complete: \(snapshot.count) file(s) written.")

    var next = state
    next.local_snapshot = localSnapshot
    next.last_server_change_id = response.latest_change_id
    next.last_server_change_unix_ms = response.latest_change_unix_ms
    return next
}

private func runDeltaPush(options: CLIOptions, api: VaultAPIClient, state: SyncState) async throws -> SyncState {
    let currentSnapshot = try scanLocalVault(root: options.localPath)
    var latestChangeIDFromStatus: Int64?
    var latestChangeUnixMSFromStatus: Int64?

    let changes: [DeltaChangePayload]
    do {
        let statusPayload = StatusPayload(vault_uid: options.vaultUID, since_unix_ms: nil, limit: options.limit)
        let status: StatusResponse = try await api.post(path: "/vaults/sync/status", body: statusPayload)
        latestChangeIDFromStatus = status.latest_change_id
        latestChangeUnixMSFromStatus = status.latest_change_unix_ms

        changes = try buildDeltaChangesUsingRemoteStatus(
            localSnapshot: currentSnapshot,
            remoteFileTimestamps: status.file_timestamps,
            localRoot: options.localPath,
            forceFullMirror: false
        )
        info("Remote status loaded: file_index=\(status.file_timestamps.count), changelog=\(status.change_log.count).")
    } catch {
        warn("Failed to load /vaults/sync/status, falling back to local state diff: \(error.localizedDescription)")
        changes = try buildDeltaChanges(
            previous: state.local_snapshot,
            current: currentSnapshot,
            localRoot: options.localPath
        )
    }

    if changes.isEmpty {
        info("No local changes detected; skipping delta push.")
        var next = state
        next.local_snapshot = currentSnapshot
        if let latestChangeIDFromStatus {
            next.last_server_change_id = latestChangeIDFromStatus
        }
        if let latestChangeUnixMSFromStatus {
            next.last_server_change_unix_ms = latestChangeUnixMSFromStatus
        }
        return next
    }

    let pushResult = try await pushDeltaChangesInBatches(
        changes: changes,
        options: options,
        api: api
    )

    info(
        "Delta push complete: sent \(pushResult.sentChanges) change(s) in \(pushResult.sentBatches) batch(es), applied \(pushResult.appliedChanges) change(s)."
    )

    var next = state
    next.local_snapshot = currentSnapshot
    next.last_server_change_id = pushResult.latestChangeID
    next.last_server_change_unix_ms = pushResult.latestChangeUnixMS
    return next
}

private func runDeltaPull(options: CLIOptions, api: VaultAPIClient, state: SyncState) async throws -> SyncState {
    guard let since = state.last_server_change_unix_ms else {
        info("No local server watermark; falling back to full-pull.")
        return try await runFullPull(options: options, api: api, state: state)
    }

    let payload = DeltaPullPayload(vault_uid: options.vaultUID, since_unix_ms: since, limit: options.limit)
    let response: PullResponse = try await api.post(path: "/vaults/sync/pull", body: payload)

    if response.mode == "delta" {
        let changedFiles = response.changed_files ?? []
        try applyDeltaChanges(changedFiles, to: options.localPath)
        info("Delta pull complete: \(changedFiles.count) change(s) applied.")
    } else {
        let snapshot = response.snapshot_files ?? []
        try applyFullSnapshot(snapshot, to: options.localPath)
        info("Delta pull returned full snapshot: \(snapshot.count) file(s) written.")
    }

    let localSnapshot = try scanLocalVault(root: options.localPath)
    var next = state
    next.local_snapshot = localSnapshot
    next.last_server_change_id = response.latest_change_id
    next.last_server_change_unix_ms = response.latest_change_unix_ms
    return next
}

private struct BatchedPushResult {
    let sentBatches: Int
    let sentChanges: Int
    let appliedChanges: Int
    let latestChangeID: Int64?
    let latestChangeUnixMS: Int64?
}

private func pushDeltaChangesInBatches(
    changes: [DeltaChangePayload],
    options: CLIOptions,
    api: VaultAPIClient
) async throws -> BatchedPushResult {
    guard !changes.isEmpty else {
        return BatchedPushResult(
            sentBatches: 0,
            sentChanges: 0,
            appliedChanges: 0,
            latestChangeID: nil,
            latestChangeUnixMS: nil
        )
    }

    var batches: [[DeltaChangePayload]] = []
    var currentBatch: [DeltaChangePayload] = []

    for change in changes {
        let candidateBatch = currentBatch + [change]
        let candidatePayload = DeltaPushPayload(vault_uid: options.vaultUID, device_id: options.deviceID, changes: candidateBatch)
        let candidateSize = try encodedByteCount(candidatePayload)

        if candidateSize <= options.maxUploadBytes {
            currentBatch = candidateBatch
            continue
        }

        if currentBatch.isEmpty {
            throw SyncScriptError.invalidArgument(
                "A single file change exceeds --max-upload-bytes (\(options.maxUploadBytes)). Increase server/body limits or split the file."
            )
        }

        batches.append(currentBatch)
        currentBatch = [change]

        let singlePayload = DeltaPushPayload(vault_uid: options.vaultUID, device_id: options.deviceID, changes: currentBatch)
        let singleSize = try encodedByteCount(singlePayload)
        if singleSize > options.maxUploadBytes {
            throw SyncScriptError.invalidArgument(
                "A single file change exceeds --max-upload-bytes (\(options.maxUploadBytes)). Increase server/body limits or split the file."
            )
        }
    }

    if !currentBatch.isEmpty {
        batches.append(currentBatch)
    }

    var appliedChanges = 0
    var latestChangeID: Int64?
    var latestChangeUnixMS: Int64?

    for (index, batch) in batches.enumerated() {
        let payload = DeltaPushPayload(vault_uid: options.vaultUID, device_id: options.deviceID, changes: batch)
        let response: PushResponse = try await api.post(path: "/vaults/sync/push", body: payload)
        appliedChanges += response.applied_changes
        latestChangeID = response.latest_change_id
        latestChangeUnixMS = response.latest_change_unix_ms
        info("Uploaded batch \(index + 1)/\(batches.count): \(batch.count) change(s).")
    }

    return BatchedPushResult(
        sentBatches: batches.count,
        sentChanges: changes.count,
        appliedChanges: appliedChanges,
        latestChangeID: latestChangeID,
        latestChangeUnixMS: latestChangeUnixMS
    )
}

private func buildDeltaChanges(
    previous: [String: LocalFileMeta],
    current: [String: LocalFileMeta],
    localRoot: URL
) throws -> [DeltaChangePayload] {
    var changes: [DeltaChangePayload] = []

    for path in current.keys.sorted() {
        let currentMeta = current[path]
        let previousMeta = previous[path]

        if previousMeta == nil {
            let data = try readLocalFile(root: localRoot, relativePath: path)
            changes.append(
                DeltaChangePayload(
                    file_path: path,
                    action: "added",
                    changed_at_unix_ms: currentMeta?.modified_unix_ms,
                    content_base64: data.base64EncodedString(),
                    content_sha256: nil
                )
            )
            continue
        }

        if previousMeta?.size_bytes != currentMeta?.size_bytes ||
            previousMeta?.modified_unix_ms != currentMeta?.modified_unix_ms
        {
            let data = try readLocalFile(root: localRoot, relativePath: path)
            changes.append(
                DeltaChangePayload(
                    file_path: path,
                    action: "modified",
                    changed_at_unix_ms: currentMeta?.modified_unix_ms,
                    content_base64: data.base64EncodedString(),
                    content_sha256: nil
                )
            )
        }
    }

    let deletedTimestamp = unixMillisecondsNow()
    for path in previous.keys.sorted() where current[path] == nil {
        changes.append(
            DeltaChangePayload(
                file_path: path,
                action: "deleted",
                changed_at_unix_ms: deletedTimestamp,
                content_base64: nil,
                content_sha256: nil
            )
        )
    }

    return changes
}

private func buildDeltaChangesUsingRemoteStatus(
    localSnapshot: [String: LocalFileMeta],
    remoteFileTimestamps: [StatusFileTimestampItem],
    localRoot: URL,
    forceFullMirror: Bool
) throws -> [DeltaChangePayload] {
    var remoteByPath: [String: StatusFileTimestampItem] = [:]
    for remote in remoteFileTimestamps {
        let normalizedPath = try normalizeRelativePath(remote.file_path)
        if shouldIgnorePath(normalizedPath) {
            continue
        }
        remoteByPath[normalizedPath] = remote
    }

    var changes: [DeltaChangePayload] = []
    var skippedAsStale = 0

    for path in localSnapshot.keys.sorted() {
        guard let localMeta = localSnapshot[path] else {
            continue
        }

        guard let remoteMeta = remoteByPath[path] else {
            let data = try readLocalFile(root: localRoot, relativePath: path)
            changes.append(
                DeltaChangePayload(
                    file_path: path,
                    action: "added",
                    changed_at_unix_ms: localMeta.modified_unix_ms,
                    content_base64: data.base64EncodedString(),
                    content_sha256: nil
                )
            )
            continue
        }

        if remoteMeta.is_deleted {
            if forceFullMirror || localMeta.modified_unix_ms > remoteMeta.updated_unix_ms {
                let data = try readLocalFile(root: localRoot, relativePath: path)
                changes.append(
                    DeltaChangePayload(
                        file_path: path,
                        action: "added",
                        changed_at_unix_ms: localMeta.modified_unix_ms,
                        content_base64: data.base64EncodedString(),
                        content_sha256: nil
                    )
                )
            } else {
                skippedAsStale += 1
            }
            continue
        }

        if !forceFullMirror && localMeta.modified_unix_ms < remoteMeta.updated_unix_ms {
            skippedAsStale += 1
            continue
        }

        if localMeta.modified_unix_ms > remoteMeta.updated_unix_ms ||
            localMeta.size_bytes != remoteMeta.size_bytes ||
            (forceFullMirror && localMeta.modified_unix_ms != remoteMeta.updated_unix_ms)
        {
            let data = try readLocalFile(root: localRoot, relativePath: path)
            changes.append(
                DeltaChangePayload(
                    file_path: path,
                    action: "modified",
                    changed_at_unix_ms: localMeta.modified_unix_ms,
                    content_base64: data.base64EncodedString(),
                    content_sha256: nil
                )
            )
        }
    }

    let deletedTimestamp = unixMillisecondsNow()
    for remote in remoteFileTimestamps {
        let normalizedPath = try normalizeRelativePath(remote.file_path)
        if shouldIgnorePath(normalizedPath) {
            continue
        }
        if remote.is_deleted {
            continue
        }
        if localSnapshot[normalizedPath] == nil {
            changes.append(
                DeltaChangePayload(
                    file_path: normalizedPath,
                    action: "deleted",
                    changed_at_unix_ms: deletedTimestamp,
                    content_base64: nil,
                    content_sha256: nil
                )
            )
        }
    }

    if skippedAsStale > 0 {
        warn("Skipped \(skippedAsStale) local file(s) because server has newer timestamps.")
    }

    return changes
}

private func applyDeltaChanges(_ changes: [ChangedFileItem], to localRoot: URL) throws {
    let fileManager = FileManager.default
    for change in changes {
        let normalizedPath = try normalizeRelativePath(change.file_path)
        if shouldIgnorePath(normalizedPath) {
            continue
        }

        let fileURL = resolvePath(root: localRoot, relativePath: normalizedPath)
        let action = change.action.lowercased()

        if action == "deleted" || action == "remove" || action == "removed" {
            if fileManager.fileExists(atPath: fileURL.path) {
                try fileManager.removeItem(at: fileURL)
            }
            continue
        }

        guard let contentBase64 = change.content_base64,
              let contentData = Data(base64Encoded: contentBase64)
        else {
            throw SyncScriptError.decoding("Missing/invalid content_base64 for changed file: \(normalizedPath)")
        }

        try fileManager.createDirectory(at: fileURL.deletingLastPathComponent(), withIntermediateDirectories: true)
        try contentData.write(to: fileURL, options: .atomic)
    }
}

private func applyFullSnapshot(_ snapshotFiles: [SnapshotFileItem], to localRoot: URL) throws {
    let fileManager = FileManager.default
    var incomingPaths = Set<String>()

    for item in snapshotFiles {
        let normalizedPath = try normalizeRelativePath(item.file_path)
        if shouldIgnorePath(normalizedPath) {
            continue
        }
        incomingPaths.insert(normalizedPath)

        guard let contentData = Data(base64Encoded: item.content_base64) else {
            throw SyncScriptError.decoding("Invalid content_base64 in snapshot for: \(normalizedPath)")
        }

        let fileURL = resolvePath(root: localRoot, relativePath: normalizedPath)
        try fileManager.createDirectory(at: fileURL.deletingLastPathComponent(), withIntermediateDirectories: true)
        try contentData.write(to: fileURL, options: .atomic)
    }

    let currentLocal = try scanLocalVault(root: localRoot)
    for existingPath in currentLocal.keys where !incomingPaths.contains(existingPath) {
        let url = resolvePath(root: localRoot, relativePath: existingPath)
        if fileManager.fileExists(atPath: url.path) {
            try fileManager.removeItem(at: url)
        }
    }
}

private func scanLocalVault(root: URL) throws -> [String: LocalFileMeta] {
    let fileManager = FileManager.default
    var snapshot: [String: LocalFileMeta] = [:]

    let keys: [URLResourceKey] = [.isRegularFileKey, .fileSizeKey, .contentModificationDateKey]
    guard let enumerator = fileManager.enumerator(
        at: root,
        includingPropertiesForKeys: keys,
        options: [],
        errorHandler: { url, error in
            FileHandle.standardError.write(Data("[vault-sync] WARN: Failed to read \(url.path): \(error.localizedDescription)\n".utf8))
            return true
        }
    ) else {
        return snapshot
    }

    for case let fileURL as URL in enumerator {
        let values = try fileURL.resourceValues(forKeys: Set(keys))
        guard values.isRegularFile == true else {
            continue
        }

        let relativePath = try relativePath(of: fileURL, from: root)
        if shouldIgnorePath(relativePath) {
            continue
        }

        let size = Int64(values.fileSize ?? 0)
        let modifiedUnixMS = Int64((values.contentModificationDate?.timeIntervalSince1970 ?? 0) * 1000.0)
        snapshot[relativePath] = LocalFileMeta(size_bytes: size, modified_unix_ms: modifiedUnixMS)
    }

    return snapshot
}

private func readLocalFile(root: URL, relativePath: String) throws -> Data {
    let normalized = try normalizeRelativePath(relativePath)
    let fileURL = resolvePath(root: root, relativePath: normalized)
    do {
        return try Data(contentsOf: fileURL)
    } catch {
        throw SyncScriptError.io("Failed to read file: \(fileURL.path)")
    }
}

private func resolvePath(root: URL, relativePath: String) -> URL {
    relativePath
        .split(separator: "/", omittingEmptySubsequences: true)
        .reduce(root) { partial, segment in
            partial.appendingPathComponent(String(segment), isDirectory: false)
        }
}

private func relativePath(of fileURL: URL, from rootURL: URL) throws -> String {
    let rootPath = rootURL.standardizedFileURL.path
    let fullPath = fileURL.standardizedFileURL.path

    if fullPath == rootPath {
        return ""
    }
    guard fullPath.hasPrefix(rootPath + "/") else {
        throw SyncScriptError.io("File path is outside local vault: \(fullPath)")
    }

    let relative = String(fullPath.dropFirst(rootPath.count + 1))
    return try normalizeRelativePath(relative)
}

private func normalizeRelativePath(_ raw: String) throws -> String {
    let trimmed = raw.trimmingCharacters(in: .whitespacesAndNewlines)
    if trimmed.isEmpty {
        throw SyncScriptError.invalidArgument("Empty file path is not allowed")
    }
    if trimmed.contains("\u{0}") {
        throw SyncScriptError.invalidArgument("File path contains null byte")
    }

    let unified = trimmed.replacingOccurrences(of: "\\", with: "/")
    if unified.hasPrefix("/") {
        throw SyncScriptError.invalidArgument("Absolute paths are not allowed: \(trimmed)")
    }

    var segments: [String] = []
    for part in unified.split(separator: "/", omittingEmptySubsequences: true) {
        if part == "." {
            continue
        }
        if part == ".." {
            throw SyncScriptError.invalidArgument("Path traversal '..' is not allowed: \(trimmed)")
        }
        segments.append(String(part))
    }

    guard !segments.isEmpty else {
        throw SyncScriptError.invalidArgument("Empty file path is not allowed")
    }
    return segments.joined(separator: "/")
}

private func validateVaultUID(_ raw: String) throws {
    let vaultUID = raw.trimmingCharacters(in: .whitespacesAndNewlines)
    if vaultUID.isEmpty {
        throw SyncScriptError.invalidArgument("--vault-uid is required")
    }
    if vaultUID.contains("\u{0}") {
        throw SyncScriptError.invalidArgument("--vault-uid contains invalid null byte")
    }
    if vaultUID.contains("/") || vaultUID.contains("\\") {
        throw SyncScriptError.invalidArgument("--vault-uid cannot contain path separators")
    }
    if vaultUID == "." || vaultUID == ".." {
        throw SyncScriptError.invalidArgument("--vault-uid cannot be '.' or '..'")
    }
}

private func shouldIgnorePath(_ relativePath: String) -> Bool {
    relativePath == ".foundation-sync/state.json" || relativePath.hasPrefix(".foundation-sync/")
}

private func ensureDirectoryExists(_ directory: URL) throws {
    try FileManager.default.createDirectory(at: directory, withIntermediateDirectories: true)
}

private func loadState(stateFile: URL, vaultUID: String) -> SyncState {
    guard let data = try? Data(contentsOf: stateFile) else {
        return SyncState(vault_uid: vaultUID, local_snapshot: [:], last_server_change_id: nil, last_server_change_unix_ms: nil)
    }
    guard let decoded = try? JSONDecoder().decode(SyncState.self, from: data) else {
        info("State file is invalid, starting with a fresh state: \(stateFile.path)")
        return SyncState(vault_uid: vaultUID, local_snapshot: [:], last_server_change_id: nil, last_server_change_unix_ms: nil)
    }
    if decoded.vault_uid != vaultUID {
        info("State file vault_uid mismatch, starting with a fresh state for \(vaultUID).")
        return SyncState(vault_uid: vaultUID, local_snapshot: [:], last_server_change_id: nil, last_server_change_unix_ms: nil)
    }
    return decoded
}

private func saveState(state: SyncState, stateFile: URL) throws {
    let fileManager = FileManager.default
    try fileManager.createDirectory(at: stateFile.deletingLastPathComponent(), withIntermediateDirectories: true)

    let encoder = JSONEncoder()
    encoder.outputFormatting = [.prettyPrinted, .sortedKeys]
    let data = try encoder.encode(state)
    try data.write(to: stateFile, options: .atomic)
}

private func unixMillisecondsNow() -> Int64 {
    Int64((Date().timeIntervalSince1970 * 1000.0).rounded())
}

private func encodedByteCount<T: Encodable>(_ payload: T) throws -> Int {
    try JSONEncoder().encode(payload).count
}

private func info(_ message: String) {
    print("[vault-sync] \(message)")
}

private func warn(_ message: String) {
    FileHandle.standardError.write(Data("[vault-sync] WARN: \(message)\n".utf8))
}

private func trimmedNonEmpty(_ raw: String?) -> String? {
    guard let raw else {
        return nil
    }
    let trimmed = raw.trimmingCharacters(in: .whitespacesAndNewlines)
    return trimmed.isEmpty ? nil : trimmed
}

private func parseInt(_ raw: String?, defaultValue: Int) -> Int {
    guard let raw else {
        return defaultValue
    }
    let trimmed = raw.trimmingCharacters(in: .whitespacesAndNewlines)
    guard let value = Int(trimmed) else {
        return defaultValue
    }
    return value
}

private func nextValue(flag: String, index: inout Int, arguments: [String]) throws -> String {
    guard index < arguments.count else {
        throw SyncScriptError.invalidArgument("Missing value for \(flag)")
    }
    let value = arguments[index]
    index += 1
    return value
}

private func printUsage() {
    print(usageText())
}

private func usageText() -> String {
    """
    Vault Sync Script (Foundation)

    Usage:
      ./client/vault_sync.swift [command] --local-path <path> [options]

    Commands:
      full-push   Upload the entire local vault as a single snapshot.
      full-pull   Download the entire remote vault snapshot and mirror locally.
      delta-push  Upload only local changes (remote timestamp/changelog aware).
      delta-pull  Pull only remote changes since last server watermark.

    Required options:
      --local-path <path>    Local vault directory path

    Optional:
      [command]              Defaults to delta-push when omitted
      --base-url <url>       Foundation base URL (default: http://localhost:8000)
      --api-key <key>        Foundation API key (default: FOUNDATION_API_KEY or host)
      --vault-uid <uid>      Vault identifier (default: FOUNDATION_VAULT_UID or folder name)
      --device-id <id>       Device identifier for changelog (default: hostname)
      --limit <n>            Max files/changes returned by pull APIs
      --max-upload-bytes <n> Max JSON payload bytes per upload request
                             (default: FOUNDATION_MAX_UPLOAD_BYTES or 8388608)
      --state-file <path>    State file path (default: <local-path>/.foundation-sync/state.json)
      -h, --help             Show this help

    Examples:
      ./client/vault_sync.swift --local-path ~/Vault
      ./client/vault_sync.swift full-push --api-key host --vault-uid my-vault --local-path ~/Vault
      ./client/vault_sync.swift full-pull --api-key host --vault-uid my-vault --local-path ~/Vault
      ./client/vault_sync.swift delta-push --api-key host --vault-uid my-vault --local-path ~/Vault
      ./client/vault_sync.swift delta-pull --api-key host --vault-uid my-vault --local-path ~/Vault
    """
}
