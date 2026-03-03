import Crypto
import FluentSQL
import Foundation

struct KeyService: Sendable {
    let initialMasterKey: String

    func listKeys(db: any SQLDatabase) async throws -> String {
        try await bootstrapMasterIfNeeded(db: db)
        let rows = try await db
            .raw("SELECT mask, created_at::text AS created_at FROM auth_keys ORDER BY created_at DESC")
            .all(decoding: KeyListRow.self)

        var output = ""
        for row in rows {
            output += "mask: \(row.mask), created_at: \(row.created_at)\n"
        }
        return output
    }

    func bootstrapMasterIfNeeded(db: any SQLDatabase) async throws {
        let rows = try await db
            .raw("SELECT COUNT(*)::bigint AS count FROM auth_keys")
            .all(decoding: CountRow.self)

        guard rows.first?.count == 0 else {
            return
        }

        let hashed = hash(apiKey: initialMasterKey)
        try await db
            .raw("INSERT INTO auth_keys (hashed_key, mask) VALUES (\(bind: hashed), \(bind: "master_key"))")
            .run()
    }

    func verify(apiKey: String, db: any SQLDatabase) async throws -> Bool {
        try await bootstrapMasterIfNeeded(db: db)
        let rows = try await db
            .raw("SELECT hashed_key FROM auth_keys")
            .all(decoding: HashedKeyRow.self)

        for row in rows where verify(storedHash: row.hashed_key, apiKey: apiKey) {
            return true
        }
        return false
    }

    func create(masterKey: String, db: any SQLDatabase) async throws -> CreateKeyResponse {
        try await bootstrapMasterIfNeeded(db: db)
        if try await !verify(apiKey: masterKey, db: db) {
            return CreateKeyResponse(ok: false, mask: nil, api_key: nil, error: "Invalid master key")
        }

        let randomKey = "foundation_" + randomString(length: 64)
        let mask = makeMask(for: randomKey)
        let hashed = hash(apiKey: randomKey)

        try await db
            .raw("INSERT INTO auth_keys (hashed_key, mask) VALUES (\(bind: hashed), \(bind: mask))")
            .run()

        return CreateKeyResponse(ok: true, mask: mask, api_key: randomKey, error: nil)
    }

    func delete(apiKey: String, db: any SQLDatabase) async throws -> Bool {
        try await bootstrapMasterIfNeeded(db: db)
        let rows = try await db
            .raw("SELECT hashed_key FROM auth_keys")
            .all(decoding: HashedKeyRow.self)

        guard let target = rows.first(where: { verify(storedHash: $0.hashed_key, apiKey: apiKey) }) else {
            return false
        }

        try await db
            .raw("DELETE FROM auth_keys WHERE hashed_key = \(bind: target.hashed_key)")
            .run()
        return true
    }

    func hash(apiKey: String) -> String {
        let salt = randomString(length: 16)
        let digest = SHA256.hash(data: Data((salt + ":" + apiKey).utf8))
        return "sha256$\(salt)$\(digest.hexString)"
    }

    private func verify(storedHash: String, apiKey: String) -> Bool {
        let parts = storedHash.split(separator: "$", maxSplits: 2, omittingEmptySubsequences: false)
        guard parts.count == 3, parts[0] == "sha256" else {
            return false
        }

        let salt = String(parts[1])
        let digest = SHA256.hash(data: Data((salt + ":" + apiKey).utf8))
        return constantTimeEquals(lhs: digest.hexString, rhs: String(parts[2]))
    }

    private func makeMask(for apiKey: String) -> String {
        let start = apiKey.index(apiKey.startIndex, offsetBy: min(11, apiKey.count))
        let end = apiKey.index(start, offsetBy: min(4, apiKey.distance(from: start, to: apiKey.endIndex)))
        return "foundation_\(apiKey[start..<end])\(String(repeating: "*", count: 60))"
    }

    private func randomString(length: Int) -> String {
        let charset = Array("abcdefghijklmnopqrstuvwxyzABCDEFGHIJKLMNOPQRSTUVWXYZ0123456789")
        var rng = SystemRandomNumberGenerator()
        return String((0..<length).map { _ in charset.randomElement(using: &rng)! })
    }

    private func constantTimeEquals(lhs: String, rhs: String) -> Bool {
        let leftBytes = Array(lhs.utf8)
        let rightBytes = Array(rhs.utf8)
        guard leftBytes.count == rightBytes.count else {
            return false
        }

        var difference: UInt8 = 0
        for index in leftBytes.indices {
            difference |= leftBytes[index] ^ rightBytes[index]
        }
        return difference == 0
    }
}

private struct CountRow: Decodable {
    let count: Int64
}

private struct HashedKeyRow: Decodable {
    let hashed_key: String
}

private struct KeyListRow: Decodable {
    let mask: String
    let created_at: String
}

private extension Digest {
    var hexString: String {
        map { String(format: "%02x", $0) }.joined()
    }
}
