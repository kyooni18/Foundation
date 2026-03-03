import Fluent
import FluentSQL
import Foundation
import Vapor

func configure(_ app: Application) throws {
    let config = AppConfig.load()
    config.configureDatabase(for: app)

    app.context = AppContext(
        config: config,
        keyService: KeyService(initialMasterKey: config.initialMasterKey),
        embeddingService: EmbeddingService(dimension: config.embeddingDimension)
    )

    try prepareSchema(app: app)
    try routes(app)
}

private func prepareSchema(app: Application) throws {
    let config = app.context.config
    let table = config.embeddingsTable
    let index = "\(table)_emb_hnsw"
    let sql = try sqlDatabase(from: app.db)

    try sql.raw("CREATE EXTENSION IF NOT EXISTS vector;").run().wait()
    try sql.raw(
        """
        CREATE TABLE IF NOT EXISTS \(ident: table) (
          id BIGSERIAL PRIMARY KEY,
          text TEXT NOT NULL,
          embedding VECTOR(\(literal: config.embeddingDimension)) NOT NULL,
          parent TEXT DEFAULT NULL,
          metadata jsonb,
          created_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
        );
        """
    ).run().wait()

    try sql.raw("CREATE INDEX IF NOT EXISTS \(ident: index) ON \(ident: table) USING hnsw (embedding vector_cosine_ops);").run().wait()

    try sql.raw(
        """
        CREATE TABLE IF NOT EXISTS auth_keys (
          id BIGSERIAL PRIMARY KEY,
          hashed_key TEXT NOT NULL,
          mask TEXT NOT NULL,
          created_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
        );
        """
    ).run().wait()

    try sql.raw(
        """
        CREATE TABLE IF NOT EXISTS archive (
          id BIGSERIAL PRIMARY KEY,
          title TEXT NOT NULL,
          content TEXT NOT NULL,
          embedding VECTOR(\(literal: config.embeddingDimension)) NOT NULL,
          atoms BIGINT[] DEFAULT ARRAY[]::BIGINT[],
          metadata jsonb,
          created_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
        );
        """
    ).run().wait()

    try sql.raw(
        """
        CREATE TABLE IF NOT EXISTS app_settings (
          setting_key TEXT PRIMARY KEY,
          setting_value TEXT NOT NULL,
          updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
        );
        """
    ).run().wait()

    try sql.raw(
        """
        CREATE TABLE IF NOT EXISTS sources (
          id BIGSERIAL PRIMARY KEY,
          source_uid TEXT NOT NULL UNIQUE,
          source_type TEXT NOT NULL,
          label TEXT,
          locator TEXT,
          metadata jsonb,
          created_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
        );
        """
    ).run().wait()

    try sql.raw(
        """
        CREATE TABLE IF NOT EXISTS source_atoms (
          source_id BIGINT NOT NULL REFERENCES sources(id) ON DELETE CASCADE,
          atom_id BIGINT NOT NULL,
          created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
          PRIMARY KEY (source_id, atom_id)
        );
        """
    ).run().wait()
    try sql.raw("CREATE INDEX IF NOT EXISTS source_atoms_atom_idx ON source_atoms (atom_id);").run().wait()

    try sql.raw(
        """
        CREATE TABLE IF NOT EXISTS source_indexes (
          source_id BIGINT PRIMARY KEY REFERENCES sources(id) ON DELETE CASCADE,
          embedding VECTOR(\(literal: config.embeddingDimension)) NOT NULL,
          atom_count INTEGER NOT NULL DEFAULT 0,
          updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
        );
        """
    ).run().wait()
    try sql.raw("CREATE INDEX IF NOT EXISTS source_indexes_emb_hnsw ON source_indexes USING hnsw (embedding vector_cosine_ops);").run().wait()

    try sql.raw(
        """
        CREATE TABLE IF NOT EXISTS source_links (
          source_id BIGINT NOT NULL REFERENCES sources(id) ON DELETE CASCADE,
          target_source_id BIGINT NOT NULL REFERENCES sources(id) ON DELETE CASCADE,
          distance DOUBLE PRECISION NOT NULL,
          method TEXT NOT NULL DEFAULT 'centroid',
          updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
          PRIMARY KEY (source_id, target_source_id),
          CHECK (source_id <> target_source_id)
        );
        """
    ).run().wait()
    try sql.raw("CREATE INDEX IF NOT EXISTS source_links_target_idx ON source_links (target_source_id);").run().wait()
}

private func routes(_ app: Application) throws {
    app.get("health") { _ in
        HealthResponse(ok: true)
    }

    app.get("health", "db") { req async throws -> HealthDBResponse in
        let sql = try sqlDatabase(from: req.db)
        let rows = try await sql
            .raw(
                "SELECT tablename FROM pg_catalog.pg_tables WHERE schemaname = 'public' ORDER BY tablename LIMIT 1"
            )
            .all(decoding: DBTableRow.self)

        return HealthDBResponse(ok: true, db: rows.first?.tablename ?? "unknown")
    }

    app.get("health", "embed") { req async throws -> HealthEmbedResponse in
        let settings = try await currentEmbeddingSettings(req: req)
        do {
            let vector = try await req.application.context.embeddingService.embed("test", settings: settings)
            return HealthEmbedResponse(ok: true, embed_dim: vector.count)
        } catch {
            throw Abort(.serviceUnavailable, reason: "embedding failed: \(error.localizedDescription)")
        }
    }

    app.get("keys", "list") { req async throws -> ListKeysResponse in
        let result = try await req.application.context.keyService.listKeys(db: try sqlDatabase(from: req.db))
        return ListKeysResponse(ok: true, result: result)
    }

    app.post("keys", "create") { req async throws -> CreateKeyResponse in
        let payload = try req.content.decode(KeyPayload.self)
        let master = payload.api_key.trimmingCharacters(in: .whitespacesAndNewlines)
        guard !master.isEmpty else {
            throw Abort(.badRequest, reason: "api_key is required")
        }

        return try await req.application.context.keyService.create(masterKey: master, db: try sqlDatabase(from: req.db))
    }

    app.post("keys", "delete") { req async throws -> DeleteKeyResponse in
        let payload = try req.content.decode(KeyPayload.self)
        let token = payload.api_key.trimmingCharacters(in: .whitespacesAndNewlines)
        guard !token.isEmpty else {
            throw Abort(.badRequest, reason: "api_key is required")
        }

        let deleted = try await req.application.context.keyService.delete(apiKey: token, db: try sqlDatabase(from: req.db))
        if deleted {
            return DeleteKeyResponse(ok: true, result: "Key deleted", error: nil)
        }
        return DeleteKeyResponse(ok: false, result: nil, error: "Key not found")
    }

    app.post("keys", "verify") { req async throws -> VerifyKeyResponse in
        let payload = try req.content.decode(KeyPayload.self)
        let token = payload.api_key.trimmingCharacters(in: .whitespacesAndNewlines)
        guard !token.isEmpty else {
            throw Abort(.badRequest, reason: "api_key is required")
        }

        let isValid = try await req.application.context.keyService.verify(apiKey: token, db: try sqlDatabase(from: req.db))
        return VerifyKeyResponse(ok: true, valid: isValid)
    }

    app.post("sources", "create") { req async throws -> SourceCreateResponse in
        try await authorize(req)
        let payload = try req.content.decode(SourceCreatePayload.self)
        let sql = try sqlDatabase(from: req.db)

        let sourceUID = nonEmpty(payload.source_uid) ?? UUID().uuidString.lowercased()
        let sourceType = try requireNonEmpty(payload.source_type, field: "source_type")
        let labelValue = nonEmpty(payload.label) ?? ""
        let locatorValue = nonEmpty(payload.locator) ?? ""
        let metadataLiteral = jsonLiteral(from: payload.metadata)
        let metadataValue = metadataLiteral ?? "null"
        let hasMetadata = metadataLiteral != nil

        do {
            let rows = try await sql
                .raw(
                    """
                    INSERT INTO sources (source_uid, source_type, label, locator, metadata)
                    VALUES (
                        \(bind: sourceUID),
                        \(bind: sourceType),
                        NULLIF(\(bind: labelValue), ''),
                        NULLIF(\(bind: locatorValue), ''),
                        NULLIF((\(bind: metadataValue))::jsonb, 'null'::jsonb)
                    )
                    ON CONFLICT (source_uid) DO UPDATE
                      SET source_type = EXCLUDED.source_type,
                          label = COALESCE(EXCLUDED.label, sources.label),
                          locator = COALESCE(EXCLUDED.locator, sources.locator),
                          metadata = CASE WHEN \(bind: hasMetadata) THEN EXCLUDED.metadata ELSE sources.metadata END
                    RETURNING id, source_uid
                    """
                )
                .all(decoding: SourceIdentityRow.self)

            guard let first = rows.first else {
                return SourceCreateResponse(ok: false, source_uid: nil, source_id: nil, error: "source create failed")
            }
            return SourceCreateResponse(ok: true, source_uid: first.source_uid, source_id: first.id, error: nil)
        } catch {
            return SourceCreateResponse(ok: false, source_uid: nil, source_id: nil, error: String(describing: error))
        }
    }

    app.get("sources", "list") { req async throws -> SourceListResponse in
        try await authorize(req)
        let sql = try sqlDatabase(from: req.db)

        let rows = try await sql
            .raw(
                """
                SELECT
                    s.source_uid,
                    s.source_type,
                    s.label,
                    s.locator,
                    s.metadata::text AS metadata,
                    s.created_at::text AS created_at,
                    COALESCE(linked.cnt, 0)::integer AS linked_atom_count,
                    COALESCE(si.atom_count, 0)::integer AS indexed_atom_count
                FROM sources s
                LEFT JOIN (
                    SELECT source_id, COUNT(*)::integer AS cnt
                    FROM source_atoms
                    GROUP BY source_id
                ) linked ON linked.source_id = s.id
                LEFT JOIN source_indexes si ON si.source_id = s.id
                ORDER BY s.created_at DESC
                LIMIT 500
                """
            )
            .all(decoding: SourceListRow.self)

        let results = rows.map {
            SourceItem(
                source_uid: $0.source_uid,
                source_type: $0.source_type,
                label: $0.label,
                locator: $0.locator,
                metadata: $0.metadata,
                created_at: $0.created_at,
                linked_atom_count: $0.linked_atom_count,
                indexed_atom_count: $0.indexed_atom_count
            )
        }

        return SourceListResponse(ok: true, results: results)
    }

    app.post("sources", "link-atom") { req async throws -> SourceLinkAtomResponse in
        try await authorize(req)
        let payload = try req.content.decode(SourceLinkAtomPayload.self)
        let sql = try sqlDatabase(from: req.db)
        let atomsTable = req.application.context.config.embeddingsTable

        let sourceUID = try requireNonEmpty(payload.source_uid, field: "source_uid")
        let source = try await resolveSourceByUID(sourceUID, sql: sql)
        let atomID = try await resolveAtomID(payload: payload, sql: sql, atomsTable: atomsTable)

        do {
            let inserted = try await sql
                .raw(
                    """
                    INSERT INTO source_atoms (source_id, atom_id)
                    VALUES (\(bind: source.id), \(bind: atomID))
                    ON CONFLICT (source_id, atom_id) DO NOTHING
                    RETURNING atom_id
                    """
                )
                .all(decoding: AtomReferenceRow.self)

            _ = try await refreshSourceIndex(sourceID: source.id, sql: sql, atomsTable: atomsTable)
            return SourceLinkAtomResponse(
                ok: true,
                source_uid: source.source_uid,
                atom_id: atomID,
                linked: !inserted.isEmpty,
                error: nil
            )
        } catch {
            return SourceLinkAtomResponse(ok: false, source_uid: source.source_uid, atom_id: atomID, linked: false, error: error.localizedDescription)
        }
    }

    app.post("sources", "unlink-atom") { req async throws -> SourceLinkAtomResponse in
        try await authorize(req)
        let payload = try req.content.decode(SourceLinkAtomPayload.self)
        let sql = try sqlDatabase(from: req.db)
        let atomsTable = req.application.context.config.embeddingsTable

        let sourceUID = try requireNonEmpty(payload.source_uid, field: "source_uid")
        let source = try await resolveSourceByUID(sourceUID, sql: sql)
        let atomID = try await resolveAtomID(payload: payload, sql: sql, atomsTable: atomsTable)

        do {
            let removed = try await sql
                .raw(
                    """
                    DELETE FROM source_atoms
                    WHERE source_id = \(bind: source.id) AND atom_id = \(bind: atomID)
                    RETURNING atom_id
                    """
                )
                .all(decoding: AtomReferenceRow.self)

            _ = try await refreshSourceIndex(sourceID: source.id, sql: sql, atomsTable: atomsTable)
            return SourceLinkAtomResponse(
                ok: true,
                source_uid: source.source_uid,
                atom_id: atomID,
                linked: !removed.isEmpty,
                error: nil
            )
        } catch {
            return SourceLinkAtomResponse(ok: false, source_uid: source.source_uid, atom_id: atomID, linked: false, error: error.localizedDescription)
        }
    }

    app.post("sources", "reindex") { req async throws -> SourceReindexResponse in
        try await authorize(req)
        let payload = try req.content.decode(SourceReindexPayload.self)
        let sql = try sqlDatabase(from: req.db)
        let atomsTable = req.application.context.config.embeddingsTable

        let sourceUID = try requireNonEmpty(payload.source_uid, field: "source_uid")
        let source = try await resolveSourceByUID(sourceUID, sql: sql)

        do {
            let atomCount = try await refreshSourceIndex(sourceID: source.id, sql: sql, atomsTable: atomsTable)
            return SourceReindexResponse(ok: true, source_uid: source.source_uid, atom_count: atomCount, error: nil)
        } catch {
            return SourceReindexResponse(ok: false, source_uid: source.source_uid, atom_count: nil, error: error.localizedDescription)
        }
    }

    app.post("sources", "find-similar") { req async throws -> SourceSimilarResponse in
        try await authorize(req)
        let payload = try req.content.decode(SourceSimilarPayload.self)
        let sql = try sqlDatabase(from: req.db)
        let atomsTable = req.application.context.config.embeddingsTable

        let sourceUID = try requireNonEmpty(payload.source_uid, field: "source_uid")
        let source = try await resolveSourceByUID(sourceUID, sql: sql)
        _ = try await refreshSourceIndex(sourceID: source.id, sql: sql, atomsTable: atomsTable)

        let limit = max(1, min(payload.limit ?? 5, 50))
        let results = try await findSourceSimilarities(sourceID: source.id, limit: limit, sql: sql)
        return SourceSimilarResponse(ok: true, source_uid: source.source_uid, results: results, error: nil)
    }

    app.post("sources", "link-similar") { req async throws -> SourceSimilarResponse in
        try await authorize(req)
        let payload = try req.content.decode(SourceSimilarPayload.self)
        let sql = try sqlDatabase(from: req.db)
        let atomsTable = req.application.context.config.embeddingsTable

        let sourceUID = try requireNonEmpty(payload.source_uid, field: "source_uid")
        let source = try await resolveSourceByUID(sourceUID, sql: sql)
        _ = try await refreshSourceIndex(sourceID: source.id, sql: sql, atomsTable: atomsTable)

        let limit = max(1, min(payload.limit ?? 5, 50))
        try await sql
            .raw(
                """
                INSERT INTO source_links (source_id, target_source_id, distance, method)
                SELECT
                    \(bind: source.id) AS source_id,
                    n.target_source_id,
                    n.distance,
                    \(bind: "centroid") AS method
                FROM (
                    SELECT
                        other.source_id AS target_source_id,
                        (base.embedding <-> other.embedding)::double precision AS distance
                    FROM source_indexes base
                    JOIN source_indexes other ON other.source_id <> base.source_id
                    WHERE base.source_id = \(bind: source.id)
                    ORDER BY base.embedding <-> other.embedding
                    LIMIT \(bind: limit)
                ) n
                ON CONFLICT (source_id, target_source_id)
                DO UPDATE SET distance = EXCLUDED.distance, method = EXCLUDED.method, updated_at = NOW()
                """
            )
            .run()

        let results = try await findSourceSimilarities(sourceID: source.id, limit: limit, sql: sql)
        return SourceSimilarResponse(ok: true, source_uid: source.source_uid, results: results, error: nil)
    }

    app.get("sources", "links", ":source_uid") { req async throws -> SourceSimilarResponse in
        try await authorize(req)
        let sql = try sqlDatabase(from: req.db)
        let sourceUID = try requireNonEmpty(req.parameters.get("source_uid"), field: "source_uid")
        let source = try await resolveSourceByUID(sourceUID, sql: sql)

        let rows = try await sql
            .raw(
                """
                SELECT
                    s.source_uid,
                    s.source_type,
                    s.label,
                    sl.distance
                FROM source_links sl
                JOIN sources s ON s.id = sl.target_source_id
                WHERE sl.source_id = \(bind: source.id)
                ORDER BY sl.distance ASC
                LIMIT 200
                """
            )
            .all(decoding: SourceDistanceRow.self)

        let results = rows.map {
            SourceDistanceItem(source_uid: $0.source_uid, source_type: $0.source_type, label: $0.label, distance: $0.distance)
        }
        return SourceSimilarResponse(ok: true, source_uid: source.source_uid, results: results, error: nil)
    }

    app.get("settings") { req async throws -> Response in
        let settings = try await currentEmbeddingSettings(req: req)
        let saved = req.query[String.self, at: "saved"] == "1"
        let body = renderSettingsHTML(settings: settings, saved: saved)

        var headers = HTTPHeaders()
        headers.contentType = .html
        return Response(status: .ok, headers: headers, body: .init(string: body))
    }

    app.post("settings") { req async throws -> Response in
        let form = try req.content.decode(SettingsForm.self)
        let sql = try sqlDatabase(from: req.db)
        let existing = try await loadEmbeddingSettings(sql: sql, defaults: req.application.context.config.defaultEmbeddingSettings)

        let provider = EmbeddingProvider(rawValue: form.provider.lowercased()) ?? .qwen3
        let qwenModel = nonEmpty(form.qwen_model) ?? existing.qwenModel
        let openAIModel = nonEmpty(form.openai_model) ?? existing.openAIModel

        let updatedOpenAIKey: String?
        if form.clear_openai_key == "1" {
            updatedOpenAIKey = nil
        } else if let postedKey = nonEmpty(form.openai_api_key) {
            updatedOpenAIKey = postedKey
        } else {
            updatedOpenAIKey = existing.openAIAPIKey
        }

        let updatedSettings = EmbeddingSettings(
            provider: provider,
            qwenModel: qwenModel,
            openAIModel: openAIModel,
            openAIAPIKey: updatedOpenAIKey
        )

        try await saveEmbeddingSettings(sql: sql, settings: updatedSettings)
        return req.redirect(to: "/settings?saved=1")
    }

    app.post("embed", "text") { req async throws -> EmbedTextResponse in
        try await authorize(req)
        let payload = try req.content.decode(TextPayload.self)
        let text = try requireText(payload.text)
        let settings = try await currentEmbeddingSettings(req: req)
        let embedding = try await req.application.context.embeddingService.embed(text, settings: settings)
        return EmbedTextResponse(ok: true, embedding: embedding)
    }

    app.post("add") { req async throws -> StandardResultResponse in
        try await authorize(req)
        let payload = try req.content.decode(TextPayload.self)
        let text = try requireText(payload.text)

        let table = req.application.context.config.embeddingsTable
        let settings = try await currentEmbeddingSettings(req: req)
        let embedding = try await req.application.context.embeddingService.embed(text, settings: settings)
        let vectorLiteral = req.application.context.embeddingService.vectorLiteral(for: embedding)
        let sql = try sqlDatabase(from: req.db)

        do {
            let existing = try await sql
                .raw("SELECT id FROM \(ident: table) WHERE text = \(bind: text) LIMIT 1")
                .all()

            if !existing.isEmpty {
                return StandardResultResponse(ok: false, result: nil, error: "Text already exists")
            }

            try await sql
                .raw("INSERT INTO \(ident: table) (text, embedding) VALUES (\(bind: text), (\(bind: vectorLiteral))::vector)")
                .run()

            let apiResult = "text: \(text), embed vectors: \(embedding.count)"
            return StandardResultResponse(ok: true, result: apiResult, error: nil)
        } catch {
            return StandardResultResponse(ok: false, result: nil, error: error.localizedDescription)
        }
    }

    app.post("delete") { req async throws -> StandardResultResponse in
        try await authorize(req)
        let payload = try req.content.decode(TextPayload.self)
        let text = try requireText(payload.text)
        let table = req.application.context.config.embeddingsTable
        let sql = try sqlDatabase(from: req.db)

        do {
            try await sql
                .raw("DELETE FROM \(ident: table) WHERE text = \(bind: text)")
                .run()
            return StandardResultResponse(ok: true, result: "Deleted text: \(text)", error: nil)
        } catch {
            return StandardResultResponse(ok: false, result: nil, error: error.localizedDescription)
        }
    }

    app.post("find") { req async throws -> FindResponse in
        try await authorize(req)
        let payload = try req.content.decode(TextPayload.self)
        let text = try requireText(payload.text)

        let table = req.application.context.config.embeddingsTable
        let settings = try await currentEmbeddingSettings(req: req)
        let embedding = try await req.application.context.embeddingService.embed(text, settings: settings)
        let vectorLiteral = req.application.context.embeddingService.vectorLiteral(for: embedding)
        let sql = try sqlDatabase(from: req.db)

        do {
            let rows = try await sql
                .raw(
                    """
                    SELECT
                        id,
                        text,
                        metadata::text AS metadata,
                        (embedding <-> (\(bind: vectorLiteral))::vector)::double precision AS distance
                    FROM \(ident: table)
                    ORDER BY embedding <-> (\(bind: vectorLiteral))::vector
                    LIMIT 5
                    """
                )
                .all(decoding: FindRow.self)

            let results = rows.map {
                FindResultItem(id: $0.id, text: $0.text, metadata: $0.metadata, distance: $0.distance)
            }
            return FindResponse(ok: true, results: results, error: nil)
        } catch {
            return FindResponse(ok: false, results: nil, error: error.localizedDescription)
        }
    }
}

private func authorize(_ req: Request) async throws {
    guard let token = req.headers.bearerAuthorization?.token else {
        throw Abort(.unauthorized, reason: "Invalid API Key")
    }

    let valid = try await req.application.context.keyService.verify(apiKey: token, db: try sqlDatabase(from: req.db))
    guard valid else {
        throw Abort(.unauthorized, reason: "Invalid API Key")
    }
}

private func sqlDatabase(from database: any Database) throws -> any SQLDatabase {
    guard let sql = database as? any SQLDatabase else {
        throw Abort(.internalServerError, reason: "SQL database is not configured")
    }
    return sql
}

private func resolveSourceByUID(_ sourceUID: String, sql: any SQLDatabase) async throws -> SourceIdentityRow {
    let rows = try await sql
        .raw(
            """
            SELECT id, source_uid
            FROM sources
            WHERE source_uid = \(bind: sourceUID)
            LIMIT 1
            """
        )
        .all(decoding: SourceIdentityRow.self)

    guard let source = rows.first else {
        throw Abort(.notFound, reason: "source not found")
    }
    return source
}

private func resolveAtomID(payload: SourceLinkAtomPayload, sql: any SQLDatabase, atomsTable: String) async throws -> Int64 {
    if let atomID = payload.atom_id {
        let rows = try await sql
            .raw("SELECT id FROM \(ident: atomsTable) WHERE id = \(bind: atomID) LIMIT 1")
            .all(decoding: AtomIDRow.self)
        guard !rows.isEmpty else {
            throw Abort(.notFound, reason: "atom not found")
        }
        return atomID
    }

    let atomText = try requireNonEmpty(payload.atom_text, field: "atom_text or atom_id")
    let rows = try await sql
        .raw("SELECT id FROM \(ident: atomsTable) WHERE text = \(bind: atomText) LIMIT 1")
        .all(decoding: AtomIDRow.self)

    guard let atom = rows.first else {
        throw Abort(.notFound, reason: "atom not found by text")
    }
    return atom.id
}

private func refreshSourceIndex(sourceID: Int64, sql: any SQLDatabase, atomsTable: String) async throws -> Int {
    let countRows = try await sql
        .raw("SELECT COUNT(*)::integer AS count FROM source_atoms WHERE source_id = \(bind: sourceID)")
        .all(decoding: IntegerCountRow.self)
    let atomCount = countRows.first?.count ?? 0

    if atomCount <= 0 {
        try await sql.raw("DELETE FROM source_indexes WHERE source_id = \(bind: sourceID)").run()
        try await sql.raw("DELETE FROM source_links WHERE source_id = \(bind: sourceID) OR target_source_id = \(bind: sourceID)").run()
        return 0
    }

    try await sql
        .raw(
            """
            INSERT INTO source_indexes (source_id, embedding, atom_count, updated_at)
            SELECT
                \(bind: sourceID) AS source_id,
                AVG(a.embedding) AS embedding,
                COUNT(*)::integer AS atom_count,
                NOW() AS updated_at
            FROM source_atoms sa
            JOIN \(ident: atomsTable) a ON a.id = sa.atom_id
            WHERE sa.source_id = \(bind: sourceID)
            ON CONFLICT (source_id)
            DO UPDATE
              SET embedding = EXCLUDED.embedding,
                  atom_count = EXCLUDED.atom_count,
                  updated_at = NOW()
            """
        )
        .run()

    return atomCount
}

private func findSourceSimilarities(sourceID: Int64, limit: Int, sql: any SQLDatabase) async throws -> [SourceDistanceItem] {
    let rows = try await sql
        .raw(
            """
            SELECT
                s.source_uid,
                s.source_type,
                s.label,
                (base.embedding <-> other.embedding)::double precision AS distance
            FROM source_indexes base
            JOIN source_indexes other ON other.source_id <> base.source_id
            JOIN sources s ON s.id = other.source_id
            WHERE base.source_id = \(bind: sourceID)
            ORDER BY base.embedding <-> other.embedding
            LIMIT \(bind: limit)
            """
        )
        .all(decoding: SourceDistanceRow.self)

    return rows.map {
        SourceDistanceItem(source_uid: $0.source_uid, source_type: $0.source_type, label: $0.label, distance: $0.distance)
    }
}

private func requireNonEmpty(_ value: String?, field: String) throws -> String {
    guard let value else {
        throw Abort(.badRequest, reason: "\(field) is required")
    }
    let trimmed = value.trimmingCharacters(in: .whitespacesAndNewlines)
    if trimmed.isEmpty {
        throw Abort(.badRequest, reason: "\(field) is required")
    }
    return trimmed
}

private func jsonLiteral(from raw: String?) -> String? {
    guard let value = nonEmpty(raw) else {
        return nil
    }

    if let data = value.data(using: .utf8),
       let object = try? JSONSerialization.jsonObject(with: data),
       let serialized = try? JSONSerialization.data(withJSONObject: object),
       let normalized = String(data: serialized, encoding: .utf8)
    {
        return normalized
    }

    if let encoded = try? JSONEncoder().encode(value),
       let literal = String(data: encoded, encoding: .utf8)
    {
        return literal
    }

    return nil
}

private func currentEmbeddingSettings(req: Request) async throws -> EmbeddingSettings {
    let sql = try sqlDatabase(from: req.db)
    return try await loadEmbeddingSettings(sql: sql, defaults: req.application.context.config.defaultEmbeddingSettings)
}

private func loadEmbeddingSettings(sql: any SQLDatabase, defaults: EmbeddingSettings) async throws -> EmbeddingSettings {
    let rows = try await sql
        .raw("SELECT setting_key, setting_value FROM app_settings")
        .all(decoding: SettingRow.self)

    var settings = defaults
    let map = Dictionary(uniqueKeysWithValues: rows.map { ($0.setting_key, $0.setting_value) })

    if let providerRaw = map["embedding_provider"], let provider = EmbeddingProvider(rawValue: providerRaw) {
        settings.provider = provider
    }
    if let qwenModel = nonEmpty(map["qwen_model"]) {
        settings.qwenModel = qwenModel
    }
    if let openAIModel = nonEmpty(map["openai_embedding_model"]) {
        settings.openAIModel = openAIModel
    }
    if let openAIKey = map["openai_api_key"] {
        settings.openAIAPIKey = nonEmpty(openAIKey)
    }

    return settings
}

private func saveEmbeddingSettings(sql: any SQLDatabase, settings: EmbeddingSettings) async throws {
    try await upsertSetting(sql: sql, key: "embedding_provider", value: settings.provider.rawValue)
    try await upsertSetting(sql: sql, key: "qwen_model", value: settings.qwenModel)
    try await upsertSetting(sql: sql, key: "openai_embedding_model", value: settings.openAIModel)
    try await upsertSetting(sql: sql, key: "openai_api_key", value: settings.openAIAPIKey ?? "")
}

private func upsertSetting(sql: any SQLDatabase, key: String, value: String) async throws {
    try await sql
        .raw(
            """
            INSERT INTO app_settings (setting_key, setting_value)
            VALUES (\(bind: key), \(bind: value))
            ON CONFLICT (setting_key)
            DO UPDATE SET setting_value = EXCLUDED.setting_value, updated_at = NOW()
            """
        )
        .run()
}

private func renderSettingsHTML(settings: EmbeddingSettings, saved: Bool) -> String {
    var modelOptions = EmbeddingSettings.availableOpenAIModels
    if !modelOptions.contains(settings.openAIModel) {
        modelOptions.append(settings.openAIModel)
    }

    let optionsHTML: String = modelOptions.map { model in
        let selected = model == settings.openAIModel ? " selected" : ""
        return "<option value=\"" + escapeHTML(model) + "\"" + selected + ">" + escapeHTML(model) + "</option>"
    }.joined(separator: "")

    let providerQwenSelected = settings.provider == .qwen3 ? " selected" : ""
    let providerOpenAISelected = settings.provider == .openai ? " selected" : ""

    let savedBanner = saved ? "<div class=\"ok\">Settings saved.</div>" : ""

    return """
    <!doctype html>
    <html lang="en">
      <head>
        <meta charset="utf-8" />
        <meta name="viewport" content="width=device-width, initial-scale=1" />
        <title>Foundation Settings</title>
        <style>
          :root { --bg:#f4f6fb; --card:#ffffff; --ink:#1f2a44; --line:#dce3f1; --accent:#0e6ac7; }
          * { box-sizing: border-box; }
          body { margin:0; font-family: -apple-system, BlinkMacSystemFont, "Segoe UI", sans-serif; background:var(--bg); color:var(--ink); }
          .wrap { max-width: 740px; margin: 32px auto; padding: 0 16px; }
          .card { background:var(--card); border:1px solid var(--line); border-radius:14px; padding:22px; }
          h1 { margin:0 0 12px; font-size: 1.5rem; }
          p { margin:0 0 16px; color:#4a5879; }
          .ok { margin:0 0 14px; padding:10px 12px; border-radius:10px; background:#eaf6ec; color:#1c6e2f; border:1px solid #b4e0bc; }
          label { display:block; margin:14px 0 6px; font-weight:600; }
          input, select { width:100%; padding:10px; border:1px solid var(--line); border-radius:10px; font-size:0.95rem; }
          .hint { margin-top:6px; font-size:0.85rem; color:#607093; }
          .row { display:grid; grid-template-columns: 1fr; gap: 12px; }
          .submit { margin-top:16px; width:auto; border:0; background:var(--accent); color:#fff; padding:10px 16px; border-radius:10px; cursor:pointer; }
          .subtle { margin-top:16px; font-size:0.9rem; }
          @media (min-width: 720px) { .row { grid-template-columns: 1fr 1fr; } }
        </style>
      </head>
      <body>
        <div class="wrap">
          <div class="card">
            <h1>Foundation Settings</h1>
            <p>Configure embedding provider and model selection for <code>/embed/text</code>, <code>/add</code>, and <code>/find</code>.</p>
            \(savedBanner)
            <form method="post" action="/settings">
              <label for="provider">Embedding provider</label>
              <select id="provider" name="provider">
                <option value="qwen3"\(providerQwenSelected)>\(EmbeddingProvider.qwen3.displayName)</option>
                <option value="openai"\(providerOpenAISelected)>\(EmbeddingProvider.openai.displayName)</option>
              </select>

              <div class="row">
                <div>
                  <label for="qwen_model">Qwen model label</label>
                  <input id="qwen_model" name="qwen_model" value="\(escapeHTML(settings.qwenModel))" />
                  <div class="hint">Label only. Local deterministic embedding backend remains active for Qwen mode.</div>
                </div>
                <div>
                  <label for="openai_model">OpenAI embedding model</label>
                  <select id="openai_model" name="openai_model">\(optionsHTML)</select>
                </div>
              </div>

              <label for="openai_api_key">OpenAI API key</label>
              <input id="openai_api_key" name="openai_api_key" placeholder="\(escapeHTML(maskAPIKey(settings.openAIAPIKey)))" />
              <div class="hint">Leave empty to keep existing key. Check box below to clear saved key.</div>

              <label>
                <input type="checkbox" name="clear_openai_key" value="1" style="width:auto; margin-right:8px;" />
                Clear stored OpenAI API key
              </label>

              <button class="submit" type="submit">Save settings</button>
            </form>
            <div class="subtle">OpenAI vectors are resized to your configured DB dimension for compatibility.</div>
          </div>
        </div>
      </body>
    </html>
    """
}

private func maskAPIKey(_ key: String?) -> String {
    guard let key, !key.isEmpty else {
        return "(not set)"
    }
    if key.count <= 8 {
        return "********"
    }

    let start = key.prefix(4)
    let end = key.suffix(4)
    return "\(start)********\(end)"
}

private func escapeHTML(_ raw: String) -> String {
    raw
        .replacingOccurrences(of: "&", with: "&amp;")
        .replacingOccurrences(of: "<", with: "&lt;")
        .replacingOccurrences(of: ">", with: "&gt;")
        .replacingOccurrences(of: "\"", with: "&quot;")
        .replacingOccurrences(of: "'", with: "&#39;")
}

private func nonEmpty(_ raw: String?) -> String? {
    guard let raw else {
        return nil
    }
    let trimmed = raw.trimmingCharacters(in: .whitespacesAndNewlines)
    return trimmed.isEmpty ? nil : trimmed
}

private func requireText(_ text: String) throws -> String {
    let trimmed = text.trimmingCharacters(in: .whitespacesAndNewlines)
    if trimmed.isEmpty {
        throw Abort(.badRequest, reason: "text is required")
    }
    return trimmed
}

private struct DBTableRow: Decodable {
    let tablename: String
}

private struct FindRow: Decodable {
    let id: Int64
    let text: String
    let metadata: String?
    let distance: Double
}

private struct SourceIdentityRow: Decodable {
    let id: Int64
    let source_uid: String
}

private struct AtomIDRow: Decodable {
    let id: Int64
}

private struct AtomReferenceRow: Decodable {
    let atom_id: Int64
}

private struct IntegerCountRow: Decodable {
    let count: Int
}

private struct SourceListRow: Decodable {
    let source_uid: String
    let source_type: String
    let label: String?
    let locator: String?
    let metadata: String?
    let created_at: String
    let linked_atom_count: Int
    let indexed_atom_count: Int
}

private struct SourceDistanceRow: Decodable {
    let source_uid: String
    let source_type: String
    let label: String?
    let distance: Double
}

private struct SettingRow: Decodable {
    let setting_key: String
    let setting_value: String
}

private struct SettingsForm: Content {
    let provider: String
    let qwen_model: String?
    let openai_model: String?
    let openai_api_key: String?
    let clear_openai_key: String?
}
