import Foundation
#if canImport(FoundationNetworking)
import FoundationNetworking
#endif

public protocol FoundationAPIStatusResponse {
    var ok: Bool { get }
    var error: String? { get }
}

public enum FoundationAPIError: Error, LocalizedError {
    case invalidURL(String)
    case invalidResponse
    case missingAPIKey
    case invalidInput(String)
    case httpStatus(code: Int, message: String)
    case api(message: String)
    case decoding(Error)

    public var errorDescription: String? {
        switch self {
        case let .invalidURL(path):
            return "Invalid URL path: \(path)"
        case .invalidResponse:
            return "The server returned an invalid response."
        case .missingAPIKey:
            return "This endpoint requires an API key. Set `client.apiKey` first."
        case let .invalidInput(message):
            return message
        case let .httpStatus(code, message):
            return "HTTP \(code): \(message)"
        case let .api(message):
            return "API error: \(message)"
        case let .decoding(error):
            return "Failed to decode response: \(error.localizedDescription)"
        }
    }
}

public final class FoundationAPIClient {
    public enum EmbeddingProvider: String {
        case qwen3
        case openai
    }

    public struct SettingsUpdateForm {
        public let provider: EmbeddingProvider
        public let qwenModel: String?
        public let openAIModel: String?
        public let openAIApiKey: String?
        public let clearOpenAIKey: Bool

        public init(
            provider: EmbeddingProvider,
            qwenModel: String? = nil,
            openAIModel: String? = nil,
            openAIApiKey: String? = nil,
            clearOpenAIKey: Bool = false
        ) {
            self.provider = provider
            self.qwenModel = qwenModel
            self.openAIModel = openAIModel
            self.openAIApiKey = openAIApiKey
            self.clearOpenAIKey = clearOpenAIKey
        }
    }

    public let baseURL: URL
    public var apiKey: String?

    private let session: URLSession
    private let encoder: JSONEncoder
    private let decoder: JSONDecoder

    public init(
        baseURL: URL = URL(string: "http://localhost:8000")!,
        apiKey: String? = nil,
        session: URLSession = .shared
    ) {
        self.baseURL = baseURL
        self.apiKey = apiKey
        self.session = session

        let encoder = JSONEncoder()
        self.encoder = encoder

        let decoder = JSONDecoder()
        self.decoder = decoder
    }

    // MARK: - Health

    public func health() async throws -> HealthResponse {
        try await send(path: "/health", method: .get)
    }

    public func healthDB() async throws -> HealthDBResponse {
        try await send(path: "/health/db", method: .get)
    }

    public func healthEmbed() async throws -> HealthEmbedResponse {
        try await send(path: "/health/embed", method: .get)
    }

    // MARK: - Keys

    public func listKeys() async throws -> ListKeysResponse {
        try await send(path: "/keys/list", method: .get)
    }

    public func createKey(masterKey: String) async throws -> CreateKeyResponse {
        try await send(path: "/keys/create", method: .post, body: KeyPayload(api_key: masterKey))
    }

    public func deleteKey(apiKey: String) async throws -> DeleteKeyResponse {
        try await send(path: "/keys/delete", method: .post, body: KeyPayload(api_key: apiKey))
    }

    public func verifyKey(apiKey: String) async throws -> VerifyKeyResponse {
        try await send(path: "/keys/verify", method: .post, body: KeyPayload(api_key: apiKey))
    }

    // MARK: - Settings (HTML/Form)

    public func getSettingsPageHTML() async throws -> String {
        var request = try makeRequest(path: "/settings", method: .get, requiresAuth: false)
        request.setValue("text/html", forHTTPHeaderField: "Accept")

        let (data, _) = try await performRaw(request: request)
        guard let html = String(data: data, encoding: .utf8) else {
            throw FoundationAPIError.api(message: "Failed to decode settings HTML.")
        }
        return html
    }

    public func updateSettings(_ form: SettingsUpdateForm) async throws {
        var request = try makeRequest(path: "/settings", method: .post, requiresAuth: false)
        request.setValue("application/x-www-form-urlencoded", forHTTPHeaderField: "Content-Type")

        var items: [URLQueryItem] = [
            URLQueryItem(name: "provider", value: form.provider.rawValue)
        ]

        if let qwenModel = form.qwenModel {
            items.append(URLQueryItem(name: "qwen_model", value: qwenModel))
        }
        if let openAIModel = form.openAIModel {
            items.append(URLQueryItem(name: "openai_model", value: openAIModel))
        }
        if let openAIApiKey = form.openAIApiKey {
            items.append(URLQueryItem(name: "openai_api_key", value: openAIApiKey))
        }
        if form.clearOpenAIKey {
            items.append(URLQueryItem(name: "clear_openai_key", value: "1"))
        }

        var components = URLComponents()
        components.queryItems = items
        request.httpBody = components.percentEncodedQuery?.data(using: .utf8)

        _ = try await performRaw(request: request)
    }

    // MARK: - Atom

    public func embedText(_ text: String) async throws -> EmbedTextResponse {
        try await send(path: "/embed/text", method: .post, body: TextPayload(text: text), requiresAuth: true)
    }

    public func addAtom(text: String) async throws -> StandardResultResponse {
        try await send(path: "/add", method: .post, body: TextPayload(text: text), requiresAuth: true)
    }

    public func deleteAtom(text: String) async throws -> StandardResultResponse {
        try await send(path: "/delete", method: .post, body: TextPayload(text: text), requiresAuth: true)
    }

    public func find(text: String) async throws -> FindResponse {
        try await send(path: "/find", method: .post, body: TextPayload(text: text), requiresAuth: true)
    }

    // MARK: - Sources

    public func createSource(_ request: SourceCreateRequest) async throws -> SourceCreateResponse {
        try await send(path: "/sources/create", method: .post, body: request, requiresAuth: true)
    }

    public func listSources() async throws -> SourceListResponse {
        try await send(path: "/sources/list", method: .get, requiresAuth: true)
    }

    public func linkAtom(_ request: SourceLinkAtomRequest) async throws -> SourceLinkAtomResponse {
        try request.validate()
        return try await send(path: "/sources/link-atom", method: .post, body: request, requiresAuth: true)
    }

    public func unlinkAtom(_ request: SourceLinkAtomRequest) async throws -> SourceLinkAtomResponse {
        try request.validate()
        return try await send(path: "/sources/unlink-atom", method: .post, body: request, requiresAuth: true)
    }

    public func reindexSource(sourceUID: String) async throws -> SourceReindexResponse {
        try await send(
            path: "/sources/reindex",
            method: .post,
            body: SourceReindexRequest(source_uid: sourceUID),
            requiresAuth: true
        )
    }

    public func findSimilarSources(sourceUID: String, limit: Int? = nil) async throws -> SourceSimilarResponse {
        try await send(
            path: "/sources/find-similar",
            method: .post,
            body: SourceSimilarRequest(source_uid: sourceUID, limit: limit),
            requiresAuth: true
        )
    }

    public func linkSimilarSources(sourceUID: String, limit: Int? = nil) async throws -> SourceSimilarResponse {
        try await send(
            path: "/sources/link-similar",
            method: .post,
            body: SourceSimilarRequest(source_uid: sourceUID, limit: limit),
            requiresAuth: true
        )
    }

    public func listSourceLinks(sourceUID: String) async throws -> SourceSimilarResponse {
        let encodedSourceUID = encodePathComponent(sourceUID)
        return try await send(path: "/sources/links/\(encodedSourceUID)", method: .get, requiresAuth: true)
    }

    // MARK: - Vault Sync

    public func pushVaultChanges(_ request: VaultSyncPushRequest) async throws -> VaultSyncPushResponse {
        try await send(path: "/vaults/sync/push", method: .post, body: request, requiresAuth: true)
    }

    public func pullVaultSync(vaultUID: String, sinceUnixMS: Int64? = nil, limit: Int? = nil) async throws -> VaultSyncPullResponse {
        try await send(
            path: "/vaults/sync/pull",
            method: .post,
            body: VaultSyncPullRequest(vault_uid: vaultUID, since_unix_ms: sinceUnixMS, limit: limit),
            requiresAuth: true
        )
    }

    public func fullPushVault(_ request: VaultSyncFullPushRequest) async throws -> VaultSyncPushResponse {
        try await send(path: "/vaults/sync/full-push", method: .post, body: request, requiresAuth: true)
    }

    public func fullPullVault(vaultUID: String, limit: Int? = nil) async throws -> VaultSyncPullResponse {
        try await send(
            path: "/vaults/sync/full-pull",
            method: .post,
            body: VaultSyncFullPullRequest(vault_uid: vaultUID, limit: limit),
            requiresAuth: true
        )
    }

    // MARK: - Internal request pipeline

    private enum HTTPMethod: String {
        case get = "GET"
        case post = "POST"
    }

    private struct VaporAbortErrorBody: Decodable {
        let error: Bool?
        let reason: String?
    }

    private struct GenericErrorBody: Decodable {
        let ok: Bool?
        let error: String?
        let reason: String?
    }

    private func send<Response: Decodable>(
        path: String,
        method: HTTPMethod,
        requiresAuth: Bool = false
    ) async throws -> Response {
        let request = try makeRequest(path: path, method: method, requiresAuth: requiresAuth)
        return try await decodeResponse(from: request)
    }

    private func send<Body: Encodable, Response: Decodable>(
        path: String,
        method: HTTPMethod,
        body: Body,
        requiresAuth: Bool = false
    ) async throws -> Response {
        var request = try makeRequest(path: path, method: method, requiresAuth: requiresAuth)
        request.setValue("application/json", forHTTPHeaderField: "Content-Type")
        request.httpBody = try encoder.encode(body)
        return try await decodeResponse(from: request)
    }

    private func makeRequest(path: String, method: HTTPMethod, requiresAuth: Bool) throws -> URLRequest {
        guard let url = URL(string: path, relativeTo: baseURL) else {
            throw FoundationAPIError.invalidURL(path)
        }

        var request = URLRequest(url: url)
        request.httpMethod = method.rawValue
        request.setValue("application/json", forHTTPHeaderField: "Accept")

        if requiresAuth {
            guard let apiKey, !apiKey.isEmpty else {
                throw FoundationAPIError.missingAPIKey
            }
            request.setValue("Bearer \(apiKey)", forHTTPHeaderField: "Authorization")
        }

        return request
    }

    private func performRaw(request: URLRequest) async throws -> (Data, HTTPURLResponse) {
        let (data, response) = try await session.data(for: request)
        guard let http = response as? HTTPURLResponse else {
            throw FoundationAPIError.invalidResponse
        }

        guard (200..<300).contains(http.statusCode) else {
            let message = decodeErrorMessage(from: data)
            throw FoundationAPIError.httpStatus(code: http.statusCode, message: message)
        }

        return (data, http)
    }

    private func decodeResponse<Response: Decodable>(from request: URLRequest) async throws -> Response {
        let (data, _) = try await performRaw(request: request)

        do {
            let decoded = try decoder.decode(Response.self, from: data)

            if let status = decoded as? FoundationAPIStatusResponse, status.ok == false {
                throw FoundationAPIError.api(message: status.error ?? "Request failed.")
            }

            return decoded
        } catch let error as FoundationAPIError {
            throw error
        } catch {
            throw FoundationAPIError.decoding(error)
        }
    }

    private func decodeErrorMessage(from data: Data) -> String {
        if let body = try? decoder.decode(GenericErrorBody.self, from: data) {
            if let message = body.error, !message.isEmpty {
                return message
            }
            if let message = body.reason, !message.isEmpty {
                return message
            }
        }

        if let abortBody = try? decoder.decode(VaporAbortErrorBody.self, from: data),
           let reason = abortBody.reason,
           !reason.isEmpty {
            return reason
        }

        if let text = String(data: data, encoding: .utf8), !text.isEmpty {
            return text
        }

        return "Unknown server error."
    }

    private func encodePathComponent(_ value: String) -> String {
        var allowed = CharacterSet.urlPathAllowed
        allowed.remove(charactersIn: "/")
        return value.addingPercentEncoding(withAllowedCharacters: allowed) ?? value
    }
}

// MARK: - Request Models

public struct TextPayload: Encodable {
    public let text: String

    public init(text: String) {
        self.text = text
    }
}

public struct KeyPayload: Encodable {
    public let api_key: String

    public init(api_key: String) {
        self.api_key = api_key
    }
}

public struct SourceCreateRequest: Encodable {
    public let source_uid: String?
    public let source_type: String
    public let label: String?
    public let locator: String?
    public let metadata: String?

    public init(
        source_uid: String? = nil,
        source_type: String,
        label: String? = nil,
        locator: String? = nil,
        metadata: String? = nil
    ) {
        self.source_uid = source_uid
        self.source_type = source_type
        self.label = label
        self.locator = locator
        self.metadata = metadata
    }
}

public struct SourceLinkAtomRequest: Encodable {
    public let source_uid: String
    public let atom_id: Int64?
    public let atom_text: String?

    public init(source_uid: String, atom_id: Int64? = nil, atom_text: String? = nil) {
        self.source_uid = source_uid
        self.atom_id = atom_id
        self.atom_text = atom_text
    }

    fileprivate func validate() throws {
        if atom_id == nil && (atom_text?.isEmpty ?? true) {
            throw FoundationAPIError.invalidInput("Either `atom_id` or non-empty `atom_text` is required.")
        }
    }
}

public struct SourceReindexRequest: Encodable {
    public let source_uid: String

    public init(source_uid: String) {
        self.source_uid = source_uid
    }
}

public struct SourceSimilarRequest: Encodable {
    public let source_uid: String
    public let limit: Int?

    public init(source_uid: String, limit: Int? = nil) {
        self.source_uid = source_uid
        self.limit = limit
    }
}

public struct VaultSyncChangeRequest: Encodable {
    public let file_path: String
    public let action: String
    public let changed_at_unix_ms: Int64?
    public let content_base64: String?
    public let content_sha256: String?

    public init(
        file_path: String,
        action: String,
        changed_at_unix_ms: Int64? = nil,
        content_base64: String? = nil,
        content_sha256: String? = nil
    ) {
        self.file_path = file_path
        self.action = action
        self.changed_at_unix_ms = changed_at_unix_ms
        self.content_base64 = content_base64
        self.content_sha256 = content_sha256
    }
}

public struct VaultSyncPushRequest: Encodable {
    public let vault_uid: String
    public let device_id: String?
    public let changes: [VaultSyncChangeRequest]

    public init(vault_uid: String, device_id: String? = nil, changes: [VaultSyncChangeRequest]) {
        self.vault_uid = vault_uid
        self.device_id = device_id
        self.changes = changes
    }
}

public struct VaultSyncPullRequest: Encodable {
    public let vault_uid: String
    public let since_unix_ms: Int64?
    public let limit: Int?

    public init(vault_uid: String, since_unix_ms: Int64? = nil, limit: Int? = nil) {
        self.vault_uid = vault_uid
        self.since_unix_ms = since_unix_ms
        self.limit = limit
    }
}

public struct VaultSyncFullFileRequest: Encodable {
    public let file_path: String
    public let content_base64: String

    public init(file_path: String, content_base64: String) {
        self.file_path = file_path
        self.content_base64 = content_base64
    }
}

public struct VaultSyncFullPushRequest: Encodable {
    public let vault_uid: String
    public let device_id: String?
    public let uploaded_at_unix_ms: Int64?
    public let files: [VaultSyncFullFileRequest]

    public init(
        vault_uid: String,
        device_id: String? = nil,
        uploaded_at_unix_ms: Int64? = nil,
        files: [VaultSyncFullFileRequest]
    ) {
        self.vault_uid = vault_uid
        self.device_id = device_id
        self.uploaded_at_unix_ms = uploaded_at_unix_ms
        self.files = files
    }
}

public struct VaultSyncFullPullRequest: Encodable {
    public let vault_uid: String
    public let limit: Int?

    public init(vault_uid: String, limit: Int? = nil) {
        self.vault_uid = vault_uid
        self.limit = limit
    }
}

// MARK: - Response Models

public struct HealthResponse: Decodable, FoundationAPIStatusResponse {
    public let ok: Bool
    public var error: String? { nil }
}

public struct HealthDBResponse: Decodable, FoundationAPIStatusResponse {
    public let ok: Bool
    public let db: String
    public var error: String? { nil }
}

public struct HealthEmbedResponse: Decodable, FoundationAPIStatusResponse {
    public let ok: Bool
    public let embedDim: Int

    public var error: String? { nil }

    enum CodingKeys: String, CodingKey {
        case ok
        case embedDim = "embed_dim"
    }
}

public struct ListKeysResponse: Decodable, FoundationAPIStatusResponse {
    public let ok: Bool
    public let result: String
    public var error: String? { nil }
}

public struct CreateKeyResponse: Decodable, FoundationAPIStatusResponse {
    public let ok: Bool
    public let mask: String?
    public let apiKey: String?
    public let error: String?

    enum CodingKeys: String, CodingKey {
        case ok
        case mask
        case apiKey = "api_key"
        case error
    }
}

public struct DeleteKeyResponse: Decodable, FoundationAPIStatusResponse {
    public let ok: Bool
    public let result: String?
    public let error: String?
}

public struct VerifyKeyResponse: Decodable, FoundationAPIStatusResponse {
    public let ok: Bool
    public let valid: Bool
    public var error: String? { nil }
}

public struct EmbedTextResponse: Decodable, FoundationAPIStatusResponse {
    public let ok: Bool
    public let embedding: [Double]
    public var error: String? { nil }
}

public struct StandardResultResponse: Decodable, FoundationAPIStatusResponse {
    public let ok: Bool
    public let result: String?
    public let error: String?
}

public struct FindResultItem: Decodable {
    public let id: Int64
    public let text: String
    public let metadata: String?
    public let distance: Double
}

public struct FindResponse: Decodable, FoundationAPIStatusResponse {
    public let ok: Bool
    public let results: [FindResultItem]?
    public let error: String?
}

public struct SourceCreateResponse: Decodable, FoundationAPIStatusResponse {
    public let ok: Bool
    public let sourceUID: String?
    public let sourceID: Int64?
    public let error: String?

    enum CodingKeys: String, CodingKey {
        case ok
        case sourceUID = "source_uid"
        case sourceID = "source_id"
        case error
    }
}

public struct SourceLinkAtomResponse: Decodable, FoundationAPIStatusResponse {
    public let ok: Bool
    public let sourceUID: String
    public let atomID: Int64?
    public let linked: Bool
    public let error: String?

    enum CodingKeys: String, CodingKey {
        case ok
        case sourceUID = "source_uid"
        case atomID = "atom_id"
        case linked
        case error
    }
}

public struct SourceReindexResponse: Decodable, FoundationAPIStatusResponse {
    public let ok: Bool
    public let sourceUID: String
    public let atomCount: Int?
    public let error: String?

    enum CodingKeys: String, CodingKey {
        case ok
        case sourceUID = "source_uid"
        case atomCount = "atom_count"
        case error
    }
}

public struct SourceItem: Decodable {
    public let sourceUID: String
    public let sourceType: String
    public let label: String?
    public let locator: String?
    public let metadata: String?
    public let createdAt: String
    public let linkedAtomCount: Int
    public let indexedAtomCount: Int

    enum CodingKeys: String, CodingKey {
        case sourceUID = "source_uid"
        case sourceType = "source_type"
        case label
        case locator
        case metadata
        case createdAt = "created_at"
        case linkedAtomCount = "linked_atom_count"
        case indexedAtomCount = "indexed_atom_count"
    }
}

public struct SourceListResponse: Decodable, FoundationAPIStatusResponse {
    public let ok: Bool
    public let results: [SourceItem]
    public var error: String? { nil }
}

public struct SourceDistanceItem: Decodable {
    public let sourceUID: String
    public let sourceType: String
    public let label: String?
    public let distance: Double

    enum CodingKeys: String, CodingKey {
        case sourceUID = "source_uid"
        case sourceType = "source_type"
        case label
        case distance
    }
}

public struct SourceSimilarResponse: Decodable, FoundationAPIStatusResponse {
    public let ok: Bool
    public let sourceUID: String
    public let results: [SourceDistanceItem]?
    public let error: String?

    enum CodingKeys: String, CodingKey {
        case ok
        case sourceUID = "source_uid"
        case results
        case error
    }
}

public struct VaultSnapshotFileItem: Decodable {
    public let filePath: String
    public let contentBase64: String
    public let contentSHA256: String
    public let sizeBytes: Int64
    public let updatedUnixMS: Int64

    enum CodingKeys: String, CodingKey {
        case filePath = "file_path"
        case contentBase64 = "content_base64"
        case contentSHA256 = "content_sha256"
        case sizeBytes = "size_bytes"
        case updatedUnixMS = "updated_unix_ms"
    }
}

public struct VaultChangedFileItem: Decodable {
    public let filePath: String
    public let action: String
    public let changedAtUnixMS: Int64
    public let contentBase64: String?
    public let contentSHA256: String?
    public let sizeBytes: Int64?

    enum CodingKeys: String, CodingKey {
        case filePath = "file_path"
        case action
        case changedAtUnixMS = "changed_at_unix_ms"
        case contentBase64 = "content_base64"
        case contentSHA256 = "content_sha256"
        case sizeBytes = "size_bytes"
    }
}

public struct VaultChangeLogItem: Decodable {
    public let changeID: Int64
    public let filePath: String
    public let action: String
    public let changedAtUnixMS: Int64
    public let deviceID: String?

    enum CodingKeys: String, CodingKey {
        case changeID = "change_id"
        case filePath = "file_path"
        case action
        case changedAtUnixMS = "changed_at_unix_ms"
        case deviceID = "device_id"
    }
}

public struct VaultSyncPushResponse: Decodable, FoundationAPIStatusResponse {
    public let ok: Bool
    public let vaultUID: String
    public let appliedChanges: Int
    public let latestChangeID: Int64?
    public let latestChangeUnixMS: Int64?
    public let error: String?

    enum CodingKeys: String, CodingKey {
        case ok
        case vaultUID = "vault_uid"
        case appliedChanges = "applied_changes"
        case latestChangeID = "latest_change_id"
        case latestChangeUnixMS = "latest_change_unix_ms"
        case error
    }
}

public struct VaultSyncPullResponse: Decodable, FoundationAPIStatusResponse {
    public let ok: Bool
    public let vaultUID: String
    public let mode: String
    public let sinceUnixMS: Int64?
    public let latestChangeID: Int64?
    public let latestChangeUnixMS: Int64?
    public let snapshotFiles: [VaultSnapshotFileItem]?
    public let changedFiles: [VaultChangedFileItem]?
    public let changeLog: [VaultChangeLogItem]?
    public let error: String?

    enum CodingKeys: String, CodingKey {
        case ok
        case vaultUID = "vault_uid"
        case mode
        case sinceUnixMS = "since_unix_ms"
        case latestChangeID = "latest_change_id"
        case latestChangeUnixMS = "latest_change_unix_ms"
        case snapshotFiles = "snapshot_files"
        case changedFiles = "changed_files"
        case changeLog = "change_log"
        case error
    }
}
