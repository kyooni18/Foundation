import Crypto
import Foundation
#if canImport(FoundationNetworking)
import FoundationNetworking
#endif
import Vapor

enum EmbeddingProvider: String, CaseIterable, Codable, Sendable {
    case qwen3
    case openai

    var displayName: String {
        switch self {
        case .qwen3:
            return "Qwen3 (local deterministic)"
        case .openai:
            return "OpenAI"
        }
    }
}

struct EmbeddingSettings: Codable, Sendable {
    var provider: EmbeddingProvider
    var qwenModel: String
    var openAIModel: String
    var openAIAPIKey: String?

    static let availableOpenAIModels = [
        "text-embedding-3-small",
        "text-embedding-3-large",
        "text-embedding-ada-002"
    ]
}

struct EmbeddingService: Sendable {
    let dimension: Int

    init(dimension: Int) {
        self.dimension = max(1, dimension)
    }

    func embed(_ input: String, settings: EmbeddingSettings) async throws -> [Double] {
        try await embedMany([input], settings: settings).first ?? zeroVector
    }

    func embedMany(_ inputs: [String], settings: EmbeddingSettings) async throws -> [[Double]] {
        guard !inputs.isEmpty else {
            return []
        }

        let normalized = inputs.map { $0.trimmingCharacters(in: .whitespacesAndNewlines) }
        var results = Array(repeating: zeroVector, count: normalized.count)

        let nonEmptyInputs = normalized.enumerated().compactMap { index, value -> (Int, String)? in
            value.isEmpty ? nil : (index, value)
        }

        guard !nonEmptyInputs.isEmpty else {
            return results
        }

        switch settings.provider {
        case .qwen3:
            for (index, value) in nonEmptyInputs {
                results[index] = deterministicEmbed(value)
            }
        case .openai:
            let vectors = try await openAIEmbedMany(nonEmptyInputs.map(\.1), settings: settings)
            guard vectors.count == nonEmptyInputs.count else {
                throw Abort(.badGateway, reason: "OpenAI embeddings API returned an unexpected number of vectors")
            }

            for ((index, _), vector) in zip(nonEmptyInputs, vectors) {
                results[index] = vector
            }
        }

        return results
    }

    func vectorLiteral(for vector: [Double]) -> String {
        let payload = vector.map { String(format: "%.8f", $0) }.joined(separator: ",")
        return "[\(payload)]"
    }

    private func deterministicEmbed(_ input: String) -> [Double] {
        var vector = [Double](repeating: 0.0, count: dimension)
        let scalars = Array(input.unicodeScalars)

        for (index, scalar) in scalars.enumerated() {
            let payload = "\(index):\(scalar.value)"
            let digest = SHA256.hash(data: Data(payload.utf8))

            for (offset, byte) in digest.prefix(8).enumerated() {
                let position = (Int(byte) + (index * 31) + (offset * 17)) % dimension
                let magnitude = Double((byte % 64) + 1) / 64.0
                let sign = ((Int(byte) ^ offset) & 1) == 0 ? 1.0 : -1.0
                vector[position] += sign * magnitude
            }
        }

        return l2Normalize(vector)
    }

    private var zeroVector: [Double] {
        [Double](repeating: 0.0, count: dimension)
    }

    private func openAIEmbedMany(_ inputs: [String], settings: EmbeddingSettings) async throws -> [[Double]] {
        guard let apiKey = settings.openAIAPIKey?.trimmingCharacters(in: .whitespacesAndNewlines), !apiKey.isEmpty else {
            throw Abort(.badRequest, reason: "OpenAI API key is missing. Update it in /settings.")
        }

        guard let url = URL(string: "https://api.openai.com/v1/embeddings") else {
            throw Abort(.internalServerError, reason: "Invalid OpenAI URL")
        }

        var request = URLRequest(url: url)
        request.httpMethod = "POST"
        request.setValue("application/json", forHTTPHeaderField: "Content-Type")
        request.setValue("Bearer \(apiKey)", forHTTPHeaderField: "Authorization")
        request.httpBody = try JSONEncoder().encode(OpenAIEmbeddingRequest(model: settings.openAIModel, input: inputs))

        let (payload, response) = try await URLSession.shared.data(for: request)
        guard let httpResponse = response as? HTTPURLResponse else {
            throw Abort(.badGateway, reason: "No HTTP response from OpenAI embeddings API")
        }

        guard (200...299).contains(httpResponse.statusCode) else {
            let errorMessage = (try? JSONDecoder().decode(OpenAIErrorResponse.self, from: payload).error.message)
                ?? String(data: payload, encoding: .utf8)
                ?? "Unknown OpenAI API error"
            throw Abort(.badGateway, reason: "OpenAI embeddings API error: \(errorMessage)")
        }

        let decoded = try JSONDecoder().decode(OpenAIEmbeddingResponse.self, from: payload)
        let sortedItems = decoded.data.sorted { $0.index < $1.index }
        guard !sortedItems.isEmpty else {
            throw Abort(.badGateway, reason: "OpenAI embeddings API returned no vectors")
        }

        return sortedItems.map { adjustDimensionAndNormalize($0.embedding) }
    }

    private func adjustDimensionAndNormalize(_ vector: [Double]) -> [Double] {
        if vector.count == dimension {
            return l2Normalize(vector)
        }
        if vector.count > dimension {
            return l2Normalize(Array(vector.prefix(dimension)))
        }

        var padded = vector
        padded.append(contentsOf: Array(repeating: 0.0, count: dimension - vector.count))
        return l2Normalize(padded)
    }

    private func l2Normalize(_ vector: [Double]) -> [Double] {
        let norm = sqrt(vector.reduce(0.0) { $0 + ($1 * $1) })
        guard norm > 0 else {
            return vector
        }
        return vector.map { $0 / norm }
    }
}

private struct OpenAIEmbeddingRequest: Codable {
    let model: String
    let input: [String]
}

private struct OpenAIEmbeddingResponse: Decodable {
    struct Item: Decodable {
        let index: Int
        let embedding: [Double]
    }

    let data: [Item]
}

private struct OpenAIErrorResponse: Decodable {
    struct ErrorPayload: Decodable {
        let message: String
    }

    let error: ErrorPayload
}
