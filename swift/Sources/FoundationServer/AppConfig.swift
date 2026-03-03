import Foundation
import Fluent
import FluentPostgresDriver
import Vapor

struct AppConfig: Sendable {
    let databaseHost: String
    let databasePort: Int
    let databaseUser: String
    let databasePassword: String
    let databaseName: String
    let embeddingsTable: String
    let embeddingDimension: Int
    let initialMasterKey: String
    let defaultEmbeddingProvider: EmbeddingProvider
    let defaultQwenModel: String
    let defaultOpenAIModel: String
    let defaultOpenAIAPIKey: String?

    static func load() -> AppConfig {
        var host = Environment.get("DATABASE_HOST") ?? "localhost"
        var port = parseInt(Environment.get("DATABASE_PORT") ?? Environment.get("POSTGRES_PORT"), defaultValue: 5432)
        var user = Environment.get("POSTGRES_USER") ?? "foundation"
        var password = Environment.get("POSTGRES_PASSWORD") ?? "host"
        var database = Environment.get("POSTGRES_DB") ?? "foundation_db1"

        if let databaseURL = Environment.get("DATABASE_URL"),
           let components = URLComponents(string: databaseURL)
        {
            if let parsedHost = components.host, !parsedHost.isEmpty {
                host = parsedHost
            }
            if let parsedPort = components.port {
                port = parsedPort
            }
            if let parsedUser = components.user, !parsedUser.isEmpty {
                user = parsedUser
            }
            if let parsedPassword = components.password {
                password = parsedPassword
            }
            let parsedDatabase = components.path.trimmingCharacters(in: CharacterSet(charactersIn: "/"))
            if !parsedDatabase.isEmpty {
                database = parsedDatabase
            }
        }

        let rawTable = Environment.get("EMBEDDINGS_TABLE") ?? "atoms_db"
        let table = sanitizeTableName(rawTable)
        let embeddingDimension = max(1, parseInt(Environment.get("EMBEDDING_DIM"), defaultValue: 1024))
        let initialMasterKey = Environment.get("INIT_MASTER_KEY") ?? "host"

        let provider = EmbeddingProvider(rawValue: (Environment.get("EMBEDDING_PROVIDER") ?? "qwen3").lowercased()) ?? .qwen3
        let qwenModel = nonEmpty(Environment.get("QWEN_MODEL")) ?? "Qwen/Qwen3-Embedding-0.6B"
        let openAIModel = nonEmpty(Environment.get("OPENAI_EMBEDDING_MODEL")) ?? "text-embedding-3-small"
        let openAIAPIKey = nonEmpty(Environment.get("OPENAI_API_KEY"))

        return AppConfig(
            databaseHost: host,
            databasePort: port,
            databaseUser: user,
            databasePassword: password,
            databaseName: database,
            embeddingsTable: table,
            embeddingDimension: embeddingDimension,
            initialMasterKey: initialMasterKey,
            defaultEmbeddingProvider: provider,
            defaultQwenModel: qwenModel,
            defaultOpenAIModel: openAIModel,
            defaultOpenAIAPIKey: openAIAPIKey
        )
    }

    var defaultEmbeddingSettings: EmbeddingSettings {
        EmbeddingSettings(
            provider: defaultEmbeddingProvider,
            qwenModel: defaultQwenModel,
            openAIModel: defaultOpenAIModel,
            openAIAPIKey: defaultOpenAIAPIKey
        )
    }

    func configureDatabase(for app: Application) {
        let configuration = SQLPostgresConfiguration(
            hostname: databaseHost,
            port: databasePort,
            username: databaseUser,
            password: databasePassword,
            database: databaseName,
            tls: .disable
        )
        app.databases.use(.postgres(configuration: configuration), as: .psql)
    }

    private static func parseInt(_ raw: String?, defaultValue: Int) -> Int {
        guard let raw, let value = Int(raw.trimmingCharacters(in: .whitespacesAndNewlines)) else {
            return defaultValue
        }
        return value
    }

    private static func nonEmpty(_ raw: String?) -> String? {
        guard let raw else {
            return nil
        }
        let trimmed = raw.trimmingCharacters(in: .whitespacesAndNewlines)
        return trimmed.isEmpty ? nil : trimmed
    }

    private static func sanitizeTableName(_ raw: String) -> String {
        let trimmed = raw.trimmingCharacters(in: .whitespacesAndNewlines)
        guard !trimmed.isEmpty else {
            return "atoms_db"
        }

        for (idx, scalar) in trimmed.unicodeScalars.enumerated() {
            if idx == 0 {
                if !(scalar == "_" || scalar.properties.isAlphabetic) {
                    return "atoms_db"
                }
            } else if !(scalar == "_" || scalar.properties.isAlphabetic || scalar.properties.numericType != nil) {
                return "atoms_db"
            }
        }

        return trimmed
    }
}

private struct AppContextKey: StorageKey {
    typealias Value = AppContext
}

struct AppContext {
    let config: AppConfig
    let keyService: KeyService
    let embeddingService: EmbeddingService
}

extension Application {
    var context: AppContext {
        get {
            guard let context = storage[AppContextKey.self] else {
                fatalError("AppContext is not configured")
            }
            return context
        }
        set {
            storage[AppContextKey.self] = newValue
        }
    }
}
