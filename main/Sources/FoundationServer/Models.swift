import Vapor

struct TextPayload: Content {
    let text: String
}

struct KeyPayload: Content {
    let api_key: String
}

struct ListKeysResponse: Content {
    let ok: Bool
    let result: String
}

struct CreateKeyResponse: Content {
    let ok: Bool
    let mask: String?
    let api_key: String?
    let error: String?
}

struct DeleteKeyResponse: Content {
    let ok: Bool
    let result: String?
    let error: String?
}

struct VerifyKeyResponse: Content {
    let ok: Bool
    let valid: Bool
}

struct HealthResponse: Content {
    let ok: Bool
}

struct HealthDBResponse: Content {
    let ok: Bool
    let db: String
}

struct HealthEmbedResponse: Content {
    let ok: Bool
    let embed_dim: Int
}

struct EmbedTextResponse: Content {
    let ok: Bool
    let embedding: [Double]
}

struct StandardResultResponse: Content {
    let ok: Bool
    let result: String?
    let error: String?
}

struct FindResultItem: Content {
    let id: Int64
    let text: String
    let metadata: String?
    let distance: Double
}

struct FindResponse: Content {
    let ok: Bool
    let results: [FindResultItem]?
    let error: String?
}

struct SourceCreatePayload: Content {
    let source_uid: String?
    let source_type: String
    let label: String?
    let locator: String?
    let metadata: String?
}

struct SourceLinkAtomPayload: Content {
    let source_uid: String
    let atom_id: Int64?
    let atom_text: String?
}

struct SourceReindexPayload: Content {
    let source_uid: String
}

struct SourceSimilarPayload: Content {
    let source_uid: String
    let limit: Int?
}

struct SourceCreateResponse: Content {
    let ok: Bool
    let source_uid: String?
    let source_id: Int64?
    let error: String?
}

struct SourceLinkAtomResponse: Content {
    let ok: Bool
    let source_uid: String
    let atom_id: Int64?
    let linked: Bool
    let error: String?
}

struct SourceReindexResponse: Content {
    let ok: Bool
    let source_uid: String
    let atom_count: Int?
    let error: String?
}

struct SourceItem: Content {
    let source_uid: String
    let source_type: String
    let label: String?
    let locator: String?
    let metadata: String?
    let created_at: String
    let linked_atom_count: Int
    let indexed_atom_count: Int
}

struct SourceListResponse: Content {
    let ok: Bool
    let results: [SourceItem]
}

struct SourceDistanceItem: Content {
    let source_uid: String
    let source_type: String
    let label: String?
    let distance: Double
}

struct SourceSimilarResponse: Content {
    let ok: Bool
    let source_uid: String
    let results: [SourceDistanceItem]?
    let error: String?
}

struct VaultSyncPushPayload: Content {
    let vault_uid: String
    let device_id: String?
    let changes: [VaultSyncChangePayload]

    init(vault_uid: String, device_id: String?, changes: [VaultSyncChangePayload]) {
        self.vault_uid = vault_uid
        self.device_id = device_id
        self.changes = changes
    }

    private enum CodingKeys: String, CodingKey {
        case vault_uid
        case device_id
        case changes
    }

    private enum DecodingKeys: String, CodingKey {
        case vault_uid
        case vaultUid
        case device_id
        case deviceId
        case changes
    }

    init(from decoder: Decoder) throws {
        let container = try decoder.container(keyedBy: DecodingKeys.self)
        vault_uid = try container.decodeAlias(String.self, primary: .vault_uid, alternate: .vaultUid)
        device_id = try container.decodeAliasIfPresent(String.self, primary: .device_id, alternate: .deviceId)
        changes = try container.decode([VaultSyncChangePayload].self, forKey: .changes)
    }
}

struct VaultSyncChangePayload: Content {
    let file_path: String
    let action: String
    let changed_at_unix_ms: Int64?
    let content_base64: String?
    let content_sha256: String?

    init(
        file_path: String,
        action: String,
        changed_at_unix_ms: Int64?,
        content_base64: String?,
        content_sha256: String?
    ) {
        self.file_path = file_path
        self.action = action
        self.changed_at_unix_ms = changed_at_unix_ms
        self.content_base64 = content_base64
        self.content_sha256 = content_sha256
    }

    private enum CodingKeys: String, CodingKey {
        case file_path
        case action
        case changed_at_unix_ms
        case content_base64
        case content_sha256
    }

    private enum DecodingKeys: String, CodingKey {
        case file_path
        case filePath
        case action
        case changed_at_unix_ms
        case changedAtUnixMS
        case content_base64
        case contentBase64
        case content_sha256
        case contentSha256
    }

    init(from decoder: Decoder) throws {
        let container = try decoder.container(keyedBy: DecodingKeys.self)
        file_path = try container.decodeAlias(String.self, primary: .file_path, alternate: .filePath)
        action = try container.decode(String.self, forKey: .action)
        changed_at_unix_ms = try container.decodeAliasIfPresent(Int64.self, primary: .changed_at_unix_ms, alternate: .changedAtUnixMS)
        content_base64 = try container.decodeAliasIfPresent(String.self, primary: .content_base64, alternate: .contentBase64)
        content_sha256 = try container.decodeAliasIfPresent(String.self, primary: .content_sha256, alternate: .contentSha256)
    }
}

struct VaultSyncPullPayload: Content {
    let vault_uid: String
    let since_unix_ms: Int64?
    let limit: Int?

    init(vault_uid: String, since_unix_ms: Int64?, limit: Int?) {
        self.vault_uid = vault_uid
        self.since_unix_ms = since_unix_ms
        self.limit = limit
    }

    private enum CodingKeys: String, CodingKey {
        case vault_uid
        case since_unix_ms
        case limit
    }

    private enum DecodingKeys: String, CodingKey {
        case vault_uid
        case vaultUid
        case since_unix_ms
        case sinceUnixMS
        case limit
    }

    init(from decoder: Decoder) throws {
        let container = try decoder.container(keyedBy: DecodingKeys.self)
        vault_uid = try container.decodeAlias(String.self, primary: .vault_uid, alternate: .vaultUid)
        since_unix_ms = try container.decodeAliasIfPresent(Int64.self, primary: .since_unix_ms, alternate: .sinceUnixMS)
        limit = try container.decodeIfPresent(Int.self, forKey: .limit)
    }
}

struct VaultSyncStatusPayload: Content {
    let vault_uid: String
    let since_unix_ms: Int64?
    let limit: Int?

    init(vault_uid: String, since_unix_ms: Int64?, limit: Int?) {
        self.vault_uid = vault_uid
        self.since_unix_ms = since_unix_ms
        self.limit = limit
    }

    private enum CodingKeys: String, CodingKey {
        case vault_uid
        case since_unix_ms
        case limit
    }

    private enum DecodingKeys: String, CodingKey {
        case vault_uid
        case vaultUid
        case since_unix_ms
        case sinceUnixMS
        case limit
    }

    init(from decoder: Decoder) throws {
        let container = try decoder.container(keyedBy: DecodingKeys.self)
        vault_uid = try container.decodeAlias(String.self, primary: .vault_uid, alternate: .vaultUid)
        since_unix_ms = try container.decodeAliasIfPresent(Int64.self, primary: .since_unix_ms, alternate: .sinceUnixMS)
        limit = try container.decodeIfPresent(Int.self, forKey: .limit)
    }
}

struct VaultSyncFullPushPayload: Content {
    let vault_uid: String
    let device_id: String?
    let uploaded_at_unix_ms: Int64?
    let files: [VaultSyncFullFilePayload]

    init(vault_uid: String, device_id: String?, uploaded_at_unix_ms: Int64?, files: [VaultSyncFullFilePayload]) {
        self.vault_uid = vault_uid
        self.device_id = device_id
        self.uploaded_at_unix_ms = uploaded_at_unix_ms
        self.files = files
    }

    private enum CodingKeys: String, CodingKey {
        case vault_uid
        case device_id
        case uploaded_at_unix_ms
        case files
    }

    private enum DecodingKeys: String, CodingKey {
        case vault_uid
        case vaultUid
        case device_id
        case deviceId
        case uploaded_at_unix_ms
        case uploadedAtUnixMS
        case files
    }

    init(from decoder: Decoder) throws {
        let container = try decoder.container(keyedBy: DecodingKeys.self)
        vault_uid = try container.decodeAlias(String.self, primary: .vault_uid, alternate: .vaultUid)
        device_id = try container.decodeAliasIfPresent(String.self, primary: .device_id, alternate: .deviceId)
        uploaded_at_unix_ms = try container.decodeAliasIfPresent(Int64.self, primary: .uploaded_at_unix_ms, alternate: .uploadedAtUnixMS)
        files = try container.decode([VaultSyncFullFilePayload].self, forKey: .files)
    }
}

struct VaultSyncFullFilePayload: Content {
    let file_path: String
    let content_base64: String

    init(file_path: String, content_base64: String) {
        self.file_path = file_path
        self.content_base64 = content_base64
    }

    private enum CodingKeys: String, CodingKey {
        case file_path
        case content_base64
    }

    private enum DecodingKeys: String, CodingKey {
        case file_path
        case filePath
        case content_base64
        case contentBase64
    }

    init(from decoder: Decoder) throws {
        let container = try decoder.container(keyedBy: DecodingKeys.self)
        file_path = try container.decodeAlias(String.self, primary: .file_path, alternate: .filePath)
        content_base64 = try container.decodeAlias(String.self, primary: .content_base64, alternate: .contentBase64)
    }
}

struct VaultSyncFullPullPayload: Content {
    let vault_uid: String
    let limit: Int?

    init(vault_uid: String, limit: Int?) {
        self.vault_uid = vault_uid
        self.limit = limit
    }

    private enum CodingKeys: String, CodingKey {
        case vault_uid
        case limit
    }

    private enum DecodingKeys: String, CodingKey {
        case vault_uid
        case vaultUid
        case limit
    }

    init(from decoder: Decoder) throws {
        let container = try decoder.container(keyedBy: DecodingKeys.self)
        vault_uid = try container.decodeAlias(String.self, primary: .vault_uid, alternate: .vaultUid)
        limit = try container.decodeIfPresent(Int.self, forKey: .limit)
    }
}

struct VaultSemanticSearchPayload: Content {
    let vault_uid: String
    let query: String
    let limit: Int?

    init(vault_uid: String, query: String, limit: Int?) {
        self.vault_uid = vault_uid
        self.query = query
        self.limit = limit
    }

    private enum CodingKeys: String, CodingKey {
        case vault_uid
        case query
        case limit
    }

    private enum DecodingKeys: String, CodingKey {
        case vault_uid
        case vaultUid
        case query
        case limit
    }

    init(from decoder: Decoder) throws {
        let container = try decoder.container(keyedBy: DecodingKeys.self)
        vault_uid = try container.decodeAlias(String.self, primary: .vault_uid, alternate: .vaultUid)
        query = try container.decode(String.self, forKey: .query)
        limit = try container.decodeIfPresent(Int.self, forKey: .limit)
    }
}

struct VaultSnapshotFileItem: Content {
    let file_path: String
    let content_base64: String
    let content_sha256: String
    let size_bytes: Int64
    let updated_unix_ms: Int64
}

struct VaultChangedFileItem: Content {
    let file_path: String
    let action: String
    let changed_at_unix_ms: Int64
    let content_base64: String?
    let content_sha256: String?
    let size_bytes: Int64?
}

struct VaultChangeLogItem: Content {
    let change_id: Int64
    let file_path: String
    let action: String
    let changed_at_unix_ms: Int64
    let device_id: String?
}

struct VaultFileTimestampItem: Content {
    let file_path: String
    let updated_unix_ms: Int64
    let size_bytes: Int64
    let is_deleted: Bool
    let last_change_id: Int64?
}

struct VaultSyncPushResponse: Content {
    let ok: Bool
    let vault_uid: String
    let applied_changes: Int
    let latest_change_id: Int64?
    let latest_change_unix_ms: Int64?
    let error: String?
}

struct VaultSyncPullResponse: Content {
    let ok: Bool
    let vault_uid: String
    let mode: String
    let since_unix_ms: Int64?
    let latest_change_id: Int64?
    let latest_change_unix_ms: Int64?
    let snapshot_files: [VaultSnapshotFileItem]?
    let changed_files: [VaultChangedFileItem]?
    let change_log: [VaultChangeLogItem]?
    let error: String?
}

struct VaultSyncStatusResponse: Content {
    let ok: Bool
    let vault_uid: String
    let since_unix_ms: Int64?
    let latest_change_id: Int64?
    let latest_change_unix_ms: Int64?
    let file_timestamps: [VaultFileTimestampItem]
    let change_log: [VaultChangeLogItem]
    let error: String?
}

struct VaultSemanticSearchResultItem: Content {
    let file_path: String
    let title: String
    let keypoint: String
    let distance: Double
    let updated_unix_ms: Int64
    let obsidian_link: String
}

struct VaultSemanticSearchResponse: Content {
    let ok: Bool
    let vault_uid: String
    let query: String
    let results: [VaultSemanticSearchResultItem]?
    let error: String?
}

private extension KeyedDecodingContainer {
    func decodeAlias<T: Decodable>(_ type: T.Type, primary: Key, alternate: Key) throws -> T {
        if let value = try decodeIfPresent(type, forKey: primary) {
            return value
        }
        if let value = try decodeIfPresent(type, forKey: alternate) {
            return value
        }

        throw DecodingError.keyNotFound(
            primary,
            .init(codingPath: codingPath, debugDescription: "Missing value for keys \(primary.stringValue) / \(alternate.stringValue)")
        )
    }

    func decodeAliasIfPresent<T: Decodable>(_ type: T.Type, primary: Key, alternate: Key) throws -> T? {
        if let value = try decodeIfPresent(type, forKey: primary) {
            return value
        }
        return try decodeIfPresent(type, forKey: alternate)
    }
}
