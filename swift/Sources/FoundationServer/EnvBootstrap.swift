import Foundation

enum EnvBootstrap {
    static func loadDotEnvIfPresent() {
        for path in candidatePaths() {
            guard FileManager.default.fileExists(atPath: path) else {
                continue
            }
            applyEnvironment(from: path)
            return
        }
    }

    private static func candidatePaths() -> [String] {
        var paths: [String] = []
        paths.append(URL(fileURLWithPath: FileManager.default.currentDirectoryPath).appendingPathComponent(".env").path)

        let sourceURL = URL(fileURLWithPath: #filePath)
        let projectRoot = sourceURL
            .deletingLastPathComponent() // FoundationServer
            .deletingLastPathComponent() // Sources
            .deletingLastPathComponent() // swift
            .deletingLastPathComponent() // Foundation
            .path
        paths.append(URL(fileURLWithPath: projectRoot).appendingPathComponent(".env").path)

        return Array(Set(paths))
    }

    private static func applyEnvironment(from path: String) {
        guard let payload = try? String(contentsOfFile: path, encoding: .utf8) else {
            return
        }

        payload.split(whereSeparator: \.isNewline).forEach { rawLine in
            let line = rawLine.trimmingCharacters(in: .whitespacesAndNewlines)
            if line.isEmpty || line.hasPrefix("#") {
                return
            }

            let parts = line.split(separator: "=", maxSplits: 1, omittingEmptySubsequences: false)
            guard parts.count == 2 else {
                return
            }

            let key = parts[0].trimmingCharacters(in: .whitespacesAndNewlines)
            var value = parts[1].trimmingCharacters(in: .whitespacesAndNewlines)
            if value.hasPrefix("\""), value.hasSuffix("\""), value.count >= 2 {
                value.removeFirst()
                value.removeLast()
            }

            guard !key.isEmpty else {
                return
            }
            if getenv(key) == nil {
                setenv(key, value, 0)
            }
        }
    }
}
