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
