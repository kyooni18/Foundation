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
        let normalized = input.trimmingCharacters(in: .whitespacesAndNewlines)
        if normalized.isEmpty {
            return [Double](repeating: 0.0, count: dimension)
        }

        switch settings.provider {
        case .qwen3:
            return deterministicEmbed(normalized)
        case .openai:
            return try await openAIEmbed(normalized, settings: settings)
        }
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

    private func openAIEmbed(_ input: String, settings: EmbeddingSettings) async throws -> [Double] {
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
        request.httpBody = try JSONEncoder().encode(OpenAIEmbeddingRequest(model: settings.openAIModel, input: input))

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
        guard let first = decoded.data.first else {
            throw Abort(.badGateway, reason: "OpenAI embeddings API returned no vectors")
        }

        return adjustDimensionAndNormalize(first.embedding)
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
    let input: String
}

private struct OpenAIEmbeddingResponse: Decodable {
    struct Item: Decodable {
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
