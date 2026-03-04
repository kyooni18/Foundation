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
}

struct VaultSyncChangePayload: Content {
    let file_path: String
    let action: String
    let changed_at_unix_ms: Int64?
    let content_base64: String?
    let content_sha256: String?
}

struct VaultSyncPullPayload: Content {
    let vault_uid: String
    let since_unix_ms: Int64?
    let limit: Int?
}

struct VaultSyncStatusPayload: Content {
    let vault_uid: String
    let since_unix_ms: Int64?
    let limit: Int?
}

struct VaultSyncFullPushPayload: Content {
    let vault_uid: String
    let device_id: String?
    let uploaded_at_unix_ms: Int64?
    let files: [VaultSyncFullFilePayload]
}

struct VaultSyncFullFilePayload: Content {
    let file_path: String
    let content_base64: String
}

struct VaultSyncFullPullPayload: Content {
    let vault_uid: String
    let limit: Int?
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
