import Fluent
import FluentSQL
import Foundation
import Vapor

func configure(_ app: Application) throws {
    let config = AppConfig.load()
    app.routes.defaultMaxBodySize = .init(value: config.maxRequestBodyBytes)
    app.logger.info("defaultMaxBodySize=\(config.maxRequestBodyBytes) bytes")
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

    try sql.raw(
        """
        CREATE TABLE IF NOT EXISTS vaults (
          id BIGSERIAL PRIMARY KEY,
          vault_uid TEXT NOT NULL UNIQUE,
          created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
          updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
        );
        """
    ).run().wait()

    try sql.raw(
        """
        CREATE TABLE IF NOT EXISTS vault_changes (
          id BIGSERIAL PRIMARY KEY,
          vault_id BIGINT NOT NULL REFERENCES vaults(id) ON DELETE CASCADE,
          device_id TEXT,
          file_path TEXT NOT NULL,
          action TEXT NOT NULL,
          changed_at_unix_ms BIGINT NOT NULL,
          content BYTEA,
          content_sha256 TEXT,
          size_bytes BIGINT,
          received_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
          CHECK (action IN ('added', 'modified', 'deleted'))
        );
        """
    ).run().wait()
    try sql.raw("CREATE INDEX IF NOT EXISTS vault_changes_vault_time_idx ON vault_changes (vault_id, changed_at_unix_ms, id);").run().wait()
    try sql.raw("CREATE INDEX IF NOT EXISTS vault_changes_vault_id_idx ON vault_changes (vault_id, id);").run().wait()

    try sql.raw(
        """
        CREATE TABLE IF NOT EXISTS vault_files (
          id BIGSERIAL PRIMARY KEY,
          vault_id BIGINT NOT NULL REFERENCES vaults(id) ON DELETE CASCADE,
          file_path TEXT NOT NULL,
          content BYTEA,
          content_sha256 TEXT NOT NULL DEFAULT '',
          size_bytes BIGINT NOT NULL DEFAULT 0,
          is_deleted BOOLEAN NOT NULL DEFAULT FALSE,
          updated_unix_ms BIGINT NOT NULL,
          updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
          last_change_id BIGINT REFERENCES vault_changes(id) ON DELETE SET NULL,
          UNIQUE (vault_id, file_path)
        );
        """
    ).run().wait()
    try sql.raw("CREATE INDEX IF NOT EXISTS vault_files_vault_updated_idx ON vault_files (vault_id, updated_unix_ms DESC);").run().wait()
    try sql.raw("CREATE INDEX IF NOT EXISTS vault_files_vault_live_idx ON vault_files (vault_id, is_deleted, file_path);").run().wait()
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

    app.post("vaults", "sync", "push") { req async throws -> VaultSyncPushResponse in
        try await authorize(req)
        let payload = try req.content.decode(VaultSyncPushPayload.self)

        let vaultUID = try requireVaultUID(payload.vault_uid)
        let deviceID = nonEmpty(payload.device_id) ?? ""
        guard !payload.changes.isEmpty else {
            throw Abort(.badRequest, reason: "changes is required")
        }

        let nowUnixMS = unixMillisecondsNow()
        let normalizedChanges = try payload.changes.map { try normalizeVaultChange($0, defaultTimestamp: nowUnixMS) }

        let latestChange = try await req.db.transaction { tx async throws -> VaultInsertedChangeRow in
            let txSQL = try sqlDatabase(from: tx)
            let vaultID = try await ensureVaultID(vaultUID: vaultUID, sql: txSQL)
            var latestInTx: VaultInsertedChangeRow?

            for change in normalizedChanges {
                let inserted = try await txSQL
                    .raw(
                        """
                        INSERT INTO vault_changes (
                            vault_id,
                            device_id,
                            file_path,
                            action,
                            changed_at_unix_ms,
                            content,
                            content_sha256,
                            size_bytes
                        ) VALUES (
                            \(bind: vaultID),
                            NULLIF(\(bind: deviceID), ''),
                            \(bind: change.file_path),
                            \(bind: change.action.rawValue),
                            \(bind: change.changed_at_unix_ms),
                            \(bind: change.content_data),
                            NULLIF(\(bind: change.content_sha256 ?? ""), ''),
                            \(bind: change.size_bytes)
                        )
                        RETURNING id, changed_at_unix_ms
                        """
                    )
                    .all(decoding: VaultInsertedChangeRow.self)

                guard let changeRow = inserted.first else {
                    throw Abort(.internalServerError, reason: "failed to insert vault change")
                }

                latestInTx = changeRow

                if change.action == .deleted {
                    try await txSQL
                        .raw(
                            """
                            INSERT INTO vault_files (
                                vault_id,
                                file_path,
                                content,
                                content_sha256,
                                size_bytes,
                                is_deleted,
                                updated_unix_ms,
                                updated_at,
                                last_change_id
                            ) VALUES (
                                \(bind: vaultID),
                                \(bind: change.file_path),
                                NULL,
                                \(bind: ""),
                                \(bind: Int64(0)),
                                TRUE,
                                \(bind: change.changed_at_unix_ms),
                                NOW(),
                                \(bind: changeRow.id)
                            )
                            ON CONFLICT (vault_id, file_path)
                            DO UPDATE SET
                                content = NULL,
                                content_sha256 = '',
                                size_bytes = 0,
                                is_deleted = TRUE,
                                updated_unix_ms = EXCLUDED.updated_unix_ms,
                                updated_at = NOW(),
                                last_change_id = EXCLUDED.last_change_id
                            """
                        )
                        .run()
                } else {
                    try await txSQL
                        .raw(
                            """
                            INSERT INTO vault_files (
                                vault_id,
                                file_path,
                                content,
                                content_sha256,
                                size_bytes,
                                is_deleted,
                                updated_unix_ms,
                                updated_at,
                                last_change_id
                            ) VALUES (
                                \(bind: vaultID),
                                \(bind: change.file_path),
                                \(bind: change.content_data),
                                \(bind: change.content_sha256 ?? ""),
                                \(bind: change.size_bytes ?? Int64(0)),
                                FALSE,
                                \(bind: change.changed_at_unix_ms),
                                NOW(),
                                \(bind: changeRow.id)
                            )
                            ON CONFLICT (vault_id, file_path)
                            DO UPDATE SET
                                content = EXCLUDED.content,
                                content_sha256 = EXCLUDED.content_sha256,
                                size_bytes = EXCLUDED.size_bytes,
                                is_deleted = FALSE,
                                updated_unix_ms = EXCLUDED.updated_unix_ms,
                                updated_at = NOW(),
                                last_change_id = EXCLUDED.last_change_id
                            """
                        )
                        .run()
                }
            }

            guard let latestInTx else {
                throw Abort(.internalServerError, reason: "failed to resolve latest vault change")
            }
            return latestInTx
        }

        do {
            try persistVaultChangesToWorkspace(vaultUID: vaultUID, changes: normalizedChanges)
        } catch {
            return VaultSyncPushResponse(
                ok: false,
                vault_uid: vaultUID,
                applied_changes: normalizedChanges.count,
                latest_change_id: latestChange.id,
                latest_change_unix_ms: latestChange.changed_at_unix_ms,
                error: "vault DB update succeeded, but writing workspace files failed: \(error.localizedDescription)"
            )
        }

        return VaultSyncPushResponse(
            ok: true,
            vault_uid: vaultUID,
            applied_changes: normalizedChanges.count,
            latest_change_id: latestChange.id,
            latest_change_unix_ms: latestChange.changed_at_unix_ms,
            error: nil
        )
    }

    app.post("vaults", "sync", "pull") { req async throws -> VaultSyncPullResponse in
        try await authorize(req)
        let payload = try req.content.decode(VaultSyncPullPayload.self)
        let sql = try sqlDatabase(from: req.db)

        let vaultUID = try requireVaultUID(payload.vault_uid)
        let limit = max(1, min(payload.limit ?? 5000, 20000))
        let vaultID = try await resolveVaultID(vaultUID: vaultUID, sql: sql)

        if let sinceUnixMS = payload.since_unix_ms {
            if sinceUnixMS < 0 {
                throw Abort(.badRequest, reason: "since_unix_ms must be >= 0")
            }

            guard let vaultID else {
                return VaultSyncPullResponse(
                    ok: true,
                    vault_uid: vaultUID,
                    mode: "delta",
                    since_unix_ms: sinceUnixMS,
                    latest_change_id: nil,
                    latest_change_unix_ms: nil,
                    snapshot_files: nil,
                    changed_files: [],
                    change_log: [],
                    error: nil
                )
            }

            let latest = try await latestVaultChange(vaultID: vaultID, sql: sql)

            let rows = try await sql
                .raw(
                    """
                    SELECT
                        id AS change_id,
                        file_path,
                        action,
                        changed_at_unix_ms,
                        device_id,
                        encode(content, 'base64') AS content_base64,
                        content_sha256,
                        size_bytes
                    FROM vault_changes
                    WHERE vault_id = \(bind: vaultID) AND changed_at_unix_ms > \(bind: sinceUnixMS)
                    ORDER BY changed_at_unix_ms ASC, id ASC
                    LIMIT \(bind: limit)
                    """
                )
                .all(decoding: VaultPullChangeRow.self)

            let changedFiles = rows.map {
                VaultChangedFileItem(
                    file_path: $0.file_path,
                    action: $0.action,
                    changed_at_unix_ms: $0.changed_at_unix_ms,
                    content_base64: $0.content_base64,
                    content_sha256: $0.content_sha256,
                    size_bytes: $0.size_bytes
                )
            }

            let changeLog = rows.map {
                VaultChangeLogItem(
                    change_id: $0.change_id,
                    file_path: $0.file_path,
                    action: $0.action,
                    changed_at_unix_ms: $0.changed_at_unix_ms,
                    device_id: $0.device_id
                )
            }

            return VaultSyncPullResponse(
                ok: true,
                vault_uid: vaultUID,
                mode: "delta",
                since_unix_ms: sinceUnixMS,
                latest_change_id: latest?.id,
                latest_change_unix_ms: latest?.changed_at_unix_ms,
                snapshot_files: nil,
                changed_files: changedFiles,
                change_log: changeLog,
                error: nil
            )
        }

        guard let vaultID else {
            return VaultSyncPullResponse(
                ok: true,
                vault_uid: vaultUID,
                mode: "full",
                since_unix_ms: nil,
                latest_change_id: nil,
                latest_change_unix_ms: nil,
                snapshot_files: [],
                changed_files: nil,
                change_log: nil,
                error: nil
            )
        }

        let latest = try await latestVaultChange(vaultID: vaultID, sql: sql)
        let rows = try await sql
            .raw(
                """
                SELECT
                    file_path,
                    encode(content, 'base64') AS content_base64,
                    content_sha256,
                    size_bytes,
                    updated_unix_ms
                FROM vault_files
                WHERE vault_id = \(bind: vaultID) AND is_deleted = FALSE
                ORDER BY file_path ASC
                LIMIT \(bind: limit)
                """
            )
            .all(decoding: VaultSnapshotRow.self)

        let snapshotFiles = rows.map {
            VaultSnapshotFileItem(
                file_path: $0.file_path,
                content_base64: $0.content_base64,
                content_sha256: $0.content_sha256,
                size_bytes: $0.size_bytes,
                updated_unix_ms: $0.updated_unix_ms
            )
        }

        return VaultSyncPullResponse(
            ok: true,
            vault_uid: vaultUID,
            mode: "full",
            since_unix_ms: nil,
            latest_change_id: latest?.id,
            latest_change_unix_ms: latest?.changed_at_unix_ms,
            snapshot_files: snapshotFiles,
            changed_files: nil,
            change_log: nil,
            error: nil
        )
    }

    app.post("vaults", "sync", "status") { req async throws -> VaultSyncStatusResponse in
        try await authorize(req)
        let payload = try req.content.decode(VaultSyncStatusPayload.self)
        let sql = try sqlDatabase(from: req.db)

        let vaultUID = try requireVaultUID(payload.vault_uid)
        let changeLimit = max(1, min(payload.limit ?? 5000, 20000))
        let sinceUnixMS = payload.since_unix_ms
        if let sinceUnixMS, sinceUnixMS < 0 {
            throw Abort(.badRequest, reason: "since_unix_ms must be >= 0")
        }

        guard let vaultID = try await resolveVaultID(vaultUID: vaultUID, sql: sql) else {
            return VaultSyncStatusResponse(
                ok: true,
                vault_uid: vaultUID,
                since_unix_ms: sinceUnixMS,
                latest_change_id: nil,
                latest_change_unix_ms: nil,
                file_timestamps: [],
                change_log: [],
                error: nil
            )
        }

        let latest = try await latestVaultChange(vaultID: vaultID, sql: sql)

        let fileRows = try await sql
            .raw(
                """
                SELECT
                    file_path,
                    updated_unix_ms,
                    size_bytes,
                    is_deleted,
                    last_change_id
                FROM vault_files
                WHERE vault_id = \(bind: vaultID)
                ORDER BY file_path ASC
                """
            )
            .all(decoding: VaultFileTimestampRow.self)

        let fileTimestamps = fileRows.map {
            VaultFileTimestampItem(
                file_path: $0.file_path,
                updated_unix_ms: $0.updated_unix_ms,
                size_bytes: $0.size_bytes,
                is_deleted: $0.is_deleted,
                last_change_id: $0.last_change_id
            )
        }

        let changeRows: [VaultStatusChangeRow]
        if let sinceUnixMS {
            changeRows = try await sql
                .raw(
                    """
                    SELECT
                        id AS change_id,
                        file_path,
                        action,
                        changed_at_unix_ms,
                        device_id
                    FROM vault_changes
                    WHERE vault_id = \(bind: vaultID) AND changed_at_unix_ms > \(bind: sinceUnixMS)
                    ORDER BY changed_at_unix_ms ASC, id ASC
                    LIMIT \(bind: changeLimit)
                    """
                )
                .all(decoding: VaultStatusChangeRow.self)
        } else {
            let newestRows = try await sql
                .raw(
                    """
                    SELECT
                        id AS change_id,
                        file_path,
                        action,
                        changed_at_unix_ms,
                        device_id
                    FROM vault_changes
                    WHERE vault_id = \(bind: vaultID)
                    ORDER BY changed_at_unix_ms DESC, id DESC
                    LIMIT \(bind: changeLimit)
                    """
                )
                .all(decoding: VaultStatusChangeRow.self)

            changeRows = newestRows.reversed()
        }

        let changeLog = changeRows.map {
            VaultChangeLogItem(
                change_id: $0.change_id,
                file_path: $0.file_path,
                action: $0.action,
                changed_at_unix_ms: $0.changed_at_unix_ms,
                device_id: $0.device_id
            )
        }

        return VaultSyncStatusResponse(
            ok: true,
            vault_uid: vaultUID,
            since_unix_ms: sinceUnixMS,
            latest_change_id: latest?.id,
            latest_change_unix_ms: latest?.changed_at_unix_ms,
            file_timestamps: fileTimestamps,
            change_log: changeLog,
            error: nil
        )
    }

    app.post("vaults", "sync", "full-push") { req async throws -> VaultSyncPushResponse in
        try await authorize(req)
        let payload = try req.content.decode(VaultSyncFullPushPayload.self)

        let vaultUID = try requireVaultUID(payload.vault_uid)
        let deviceID = nonEmpty(payload.device_id) ?? ""
        let nowUnixMS = unixMillisecondsNow()
        let uploadedAtUnixMS = payload.uploaded_at_unix_ms ?? nowUnixMS
        if uploadedAtUnixMS < 0 {
            throw Abort(.badRequest, reason: "uploaded_at_unix_ms must be >= 0")
        }

        let normalizedFiles = try normalizeFullVaultFiles(payload.files)

        let txResult = try await req.db.transaction { tx async throws -> VaultFullPushTxResult in
            let txSQL = try sqlDatabase(from: tx)
            let vaultID = try await ensureVaultID(vaultUID: vaultUID, sql: txSQL)

            let existingRows = try await txSQL
                .raw(
                    """
                    SELECT file_path
                    FROM vault_files
                    WHERE vault_id = \(bind: vaultID) AND is_deleted = FALSE
                    """
                )
                .all(decoding: VaultFilePathRow.self)

            let existingPaths = Set(existingRows.map(\.file_path))
            let incomingPaths = Set(normalizedFiles.map(\.file_path))
            let deletedPaths = Array(existingPaths.subtracting(incomingPaths)).sorted()

            var appliedChanges = 0
            var latestInTx: VaultInsertedChangeRow?

            for file in normalizedFiles {
                let action: VaultChangeAction = existingPaths.contains(file.file_path) ? .modified : .added
                let inserted = try await txSQL
                    .raw(
                        """
                        INSERT INTO vault_changes (
                            vault_id,
                            device_id,
                            file_path,
                            action,
                            changed_at_unix_ms,
                            content,
                            content_sha256,
                            size_bytes
                        ) VALUES (
                            \(bind: vaultID),
                            NULLIF(\(bind: deviceID), ''),
                            \(bind: file.file_path),
                            \(bind: action.rawValue),
                            \(bind: uploadedAtUnixMS),
                            \(bind: file.content_data),
                            NULL,
                            \(bind: file.size_bytes)
                        )
                        RETURNING id, changed_at_unix_ms
                        """
                    )
                    .all(decoding: VaultInsertedChangeRow.self)

                guard let changeRow = inserted.first else {
                    throw Abort(.internalServerError, reason: "failed to insert vault full push change")
                }

                latestInTx = changeRow
                appliedChanges += 1

                try await txSQL
                    .raw(
                        """
                        INSERT INTO vault_files (
                            vault_id,
                            file_path,
                            content,
                            content_sha256,
                            size_bytes,
                            is_deleted,
                            updated_unix_ms,
                            updated_at,
                            last_change_id
                        ) VALUES (
                            \(bind: vaultID),
                            \(bind: file.file_path),
                            \(bind: file.content_data),
                            \(bind: ""),
                            \(bind: file.size_bytes),
                            FALSE,
                            \(bind: uploadedAtUnixMS),
                            NOW(),
                            \(bind: changeRow.id)
                        )
                        ON CONFLICT (vault_id, file_path)
                        DO UPDATE SET
                            content = EXCLUDED.content,
                            content_sha256 = EXCLUDED.content_sha256,
                            size_bytes = EXCLUDED.size_bytes,
                            is_deleted = FALSE,
                            updated_unix_ms = EXCLUDED.updated_unix_ms,
                            updated_at = NOW(),
                            last_change_id = EXCLUDED.last_change_id
                        """
                    )
                    .run()
            }

            for filePath in deletedPaths {
                let inserted = try await txSQL
                    .raw(
                        """
                        INSERT INTO vault_changes (
                            vault_id,
                            device_id,
                            file_path,
                            action,
                            changed_at_unix_ms,
                            content,
                            content_sha256,
                            size_bytes
                        ) VALUES (
                            \(bind: vaultID),
                            NULLIF(\(bind: deviceID), ''),
                            \(bind: filePath),
                            \(bind: VaultChangeAction.deleted.rawValue),
                            \(bind: uploadedAtUnixMS),
                            NULL,
                            NULL,
                            NULL
                        )
                        RETURNING id, changed_at_unix_ms
                        """
                    )
                    .all(decoding: VaultInsertedChangeRow.self)

                guard let changeRow = inserted.first else {
                    throw Abort(.internalServerError, reason: "failed to insert vault full push delete change")
                }

                latestInTx = changeRow
                appliedChanges += 1

                try await txSQL
                    .raw(
                        """
                        INSERT INTO vault_files (
                            vault_id,
                            file_path,
                            content,
                            content_sha256,
                            size_bytes,
                            is_deleted,
                            updated_unix_ms,
                            updated_at,
                            last_change_id
                        ) VALUES (
                            \(bind: vaultID),
                            \(bind: filePath),
                            NULL,
                            \(bind: ""),
                            \(bind: Int64(0)),
                            TRUE,
                            \(bind: uploadedAtUnixMS),
                            NOW(),
                            \(bind: changeRow.id)
                        )
                        ON CONFLICT (vault_id, file_path)
                        DO UPDATE SET
                            content = NULL,
                            content_sha256 = '',
                            size_bytes = 0,
                            is_deleted = TRUE,
                            updated_unix_ms = EXCLUDED.updated_unix_ms,
                            updated_at = NOW(),
                            last_change_id = EXCLUDED.last_change_id
                        """
                    )
                    .run()
            }

            return VaultFullPushTxResult(applied_changes: appliedChanges, latest_change: latestInTx)
        }

        do {
            try persistFullVaultToWorkspace(vaultUID: vaultUID, files: normalizedFiles)
        } catch {
            return VaultSyncPushResponse(
                ok: false,
                vault_uid: vaultUID,
                applied_changes: txResult.applied_changes,
                latest_change_id: txResult.latest_change?.id,
                latest_change_unix_ms: txResult.latest_change?.changed_at_unix_ms,
                error: "vault DB update succeeded, but writing workspace files failed: \(error.localizedDescription)"
            )
        }

        return VaultSyncPushResponse(
            ok: true,
            vault_uid: vaultUID,
            applied_changes: txResult.applied_changes,
            latest_change_id: txResult.latest_change?.id,
            latest_change_unix_ms: txResult.latest_change?.changed_at_unix_ms,
            error: nil
        )
    }

    app.post("vaults", "sync", "full-pull") { req async throws -> VaultSyncPullResponse in
        try await authorize(req)
        let payload = try req.content.decode(VaultSyncFullPullPayload.self)
        let sql = try sqlDatabase(from: req.db)

        let vaultUID = try requireVaultUID(payload.vault_uid)
        let limit = max(1, min(payload.limit ?? 5000, 20000))
        let vaultID = try await resolveVaultID(vaultUID: vaultUID, sql: sql)

        guard let vaultID else {
            return VaultSyncPullResponse(
                ok: true,
                vault_uid: vaultUID,
                mode: "full",
                since_unix_ms: nil,
                latest_change_id: nil,
                latest_change_unix_ms: nil,
                snapshot_files: [],
                changed_files: nil,
                change_log: nil,
                error: nil
            )
        }

        let latest = try await latestVaultChange(vaultID: vaultID, sql: sql)
        let rows = try await sql
            .raw(
                """
                SELECT
                    file_path,
                    encode(content, 'base64') AS content_base64,
                    content_sha256,
                    size_bytes,
                    updated_unix_ms
                FROM vault_files
                WHERE vault_id = \(bind: vaultID) AND is_deleted = FALSE
                ORDER BY file_path ASC
                LIMIT \(bind: limit)
                """
            )
            .all(decoding: VaultSnapshotRow.self)

        let snapshotFiles = rows.map {
            VaultSnapshotFileItem(
                file_path: $0.file_path,
                content_base64: $0.content_base64,
                content_sha256: $0.content_sha256,
                size_bytes: $0.size_bytes,
                updated_unix_ms: $0.updated_unix_ms
            )
        }

        return VaultSyncPullResponse(
            ok: true,
            vault_uid: vaultUID,
            mode: "full",
            since_unix_ms: nil,
            latest_change_id: latest?.id,
            latest_change_unix_ms: latest?.changed_at_unix_ms,
            snapshot_files: snapshotFiles,
            changed_files: nil,
            change_log: nil,
            error: nil
        )
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

private enum VaultChangeAction: String {
    case added
    case modified
    case deleted

    static func parse(_ raw: String) -> VaultChangeAction? {
        switch raw.trimmingCharacters(in: .whitespacesAndNewlines).lowercased() {
        case "added", "add", "created", "create":
            return .added
        case "modified", "modify", "changed", "updated", "update":
            return .modified
        case "deleted", "delete", "removed", "remove":
            return .deleted
        default:
            return nil
        }
    }
}

private struct NormalizedVaultChange {
    let file_path: String
    let action: VaultChangeAction
    let changed_at_unix_ms: Int64
    let content_data: Data?
    let content_sha256: String?
    let size_bytes: Int64?
}

private struct NormalizedVaultFullFile {
    let file_path: String
    let content_data: Data
    let size_bytes: Int64
}

private struct VaultFullPushTxResult {
    let applied_changes: Int
    let latest_change: VaultInsertedChangeRow?
}

private func unixMillisecondsNow() -> Int64 {
    Int64((Date().timeIntervalSince1970 * 1000.0).rounded())
}

private func normalizeVaultChange(_ payload: VaultSyncChangePayload, defaultTimestamp: Int64) throws -> NormalizedVaultChange {
    let filePath = try normalizeVaultPath(payload.file_path)
    guard let action = VaultChangeAction.parse(payload.action) else {
        throw Abort(.badRequest, reason: "action must be one of added, modified, deleted")
    }

    let changedAtUnixMS: Int64
    if let changedAt = payload.changed_at_unix_ms {
        if changedAt < 0 {
            throw Abort(.badRequest, reason: "changed_at_unix_ms must be >= 0")
        }
        changedAtUnixMS = changedAt
    } else {
        changedAtUnixMS = defaultTimestamp
    }

    if action == .deleted {
        return NormalizedVaultChange(
            file_path: filePath,
            action: action,
            changed_at_unix_ms: changedAtUnixMS,
            content_data: nil,
            content_sha256: nil,
            size_bytes: nil
        )
    }

    guard let contentBase64 = nonEmpty(payload.content_base64),
          let contentData = Data(base64Encoded: contentBase64)
    else {
        throw Abort(.badRequest, reason: "content_base64 is required for added/modified actions")
    }

    return NormalizedVaultChange(
        file_path: filePath,
        action: action,
        changed_at_unix_ms: changedAtUnixMS,
        content_data: contentData,
        content_sha256: nonEmpty(payload.content_sha256),
        size_bytes: Int64(contentData.count)
    )
}

private func normalizeFullVaultFiles(_ files: [VaultSyncFullFilePayload]) throws -> [NormalizedVaultFullFile] {
    guard !files.isEmpty else {
        return []
    }

    var seen = Set<String>()
    var normalized: [NormalizedVaultFullFile] = []
    normalized.reserveCapacity(files.count)

    for file in files {
        let filePath = try normalizeVaultPath(file.file_path)
        if seen.contains(filePath) {
            throw Abort(.badRequest, reason: "duplicate file_path in full push: \(filePath)")
        }
        seen.insert(filePath)

        guard let contentData = Data(base64Encoded: file.content_base64) else {
            throw Abort(.badRequest, reason: "invalid content_base64 for file_path \(filePath)")
        }
        normalized.append(
            NormalizedVaultFullFile(
                file_path: filePath,
                content_data: contentData,
                size_bytes: Int64(contentData.count)
            )
        )
    }

    return normalized
}

private func normalizeVaultPath(_ raw: String) throws -> String {
    let trimmed = raw.trimmingCharacters(in: .whitespacesAndNewlines)
    if trimmed.isEmpty {
        throw Abort(.badRequest, reason: "file_path is required")
    }
    if trimmed.contains("\u{0}") {
        throw Abort(.badRequest, reason: "file_path contains invalid null byte")
    }

    let unified = trimmed.replacingOccurrences(of: "\\", with: "/")
    if unified.hasPrefix("/") {
        throw Abort(.badRequest, reason: "file_path must be relative")
    }

    var parts: [String] = []
    for segment in unified.split(separator: "/", omittingEmptySubsequences: true) {
        if segment == "." {
            continue
        }
        if segment == ".." {
            throw Abort(.badRequest, reason: "file_path cannot contain '..'")
        }
        parts.append(String(segment))
    }

    guard !parts.isEmpty else {
        throw Abort(.badRequest, reason: "file_path is required")
    }

    let normalized = parts.joined(separator: "/")
    if normalized.count > 2048 {
        throw Abort(.badRequest, reason: "file_path is too long")
    }
    return normalized
}

private func ensureVaultID(vaultUID: String, sql: any SQLDatabase) async throws -> Int64 {
    let rows = try await sql
        .raw(
            """
            INSERT INTO vaults (vault_uid, updated_at)
            VALUES (\(bind: vaultUID), NOW())
            ON CONFLICT (vault_uid)
            DO UPDATE SET updated_at = NOW()
            RETURNING id
            """
        )
        .all(decoding: VaultIDRow.self)

    guard let row = rows.first else {
        throw Abort(.internalServerError, reason: "failed to resolve vault")
    }
    return row.id
}

private func resolveVaultID(vaultUID: String, sql: any SQLDatabase) async throws -> Int64? {
    let rows = try await sql
        .raw(
            """
            SELECT id
            FROM vaults
            WHERE vault_uid = \(bind: vaultUID)
            LIMIT 1
            """
        )
        .all(decoding: VaultIDRow.self)

    return rows.first?.id
}

private func latestVaultChange(vaultID: Int64, sql: any SQLDatabase) async throws -> VaultLatestChangeRow? {
    let rows = try await sql
        .raw(
            """
            SELECT id, changed_at_unix_ms
            FROM vault_changes
            WHERE vault_id = \(bind: vaultID)
            ORDER BY id DESC
            LIMIT 1
            """
        )
        .all(decoding: VaultLatestChangeRow.self)

    return rows.first
}

private func persistVaultChangesToWorkspace(vaultUID: String, changes: [NormalizedVaultChange]) throws {
    let fileManager = FileManager.default
    let vaultRoot = workspaceVaultRoot(vaultUID: vaultUID)
    try fileManager.createDirectory(at: vaultRoot, withIntermediateDirectories: true)

    for change in changes {
        let fileURL = workspaceVaultFileURL(vaultRoot: vaultRoot, filePath: change.file_path)
        switch change.action {
        case .deleted:
            if fileManager.fileExists(atPath: fileURL.path) {
                try fileManager.removeItem(at: fileURL)
            }
        case .added, .modified:
            guard let content = change.content_data else {
                throw Abort(.internalServerError, reason: "missing file content for workspace write")
            }
            try fileManager.createDirectory(at: fileURL.deletingLastPathComponent(), withIntermediateDirectories: true)
            try content.write(to: fileURL, options: .atomic)
        }
    }
}

private func persistFullVaultToWorkspace(vaultUID: String, files: [NormalizedVaultFullFile]) throws {
    let fileManager = FileManager.default
    let vaultRoot = workspaceVaultRoot(vaultUID: vaultUID)

    if fileManager.fileExists(atPath: vaultRoot.path) {
        try fileManager.removeItem(at: vaultRoot)
    }
    try fileManager.createDirectory(at: vaultRoot, withIntermediateDirectories: true)

    for file in files {
        let fileURL = workspaceVaultFileURL(vaultRoot: vaultRoot, filePath: file.file_path)
        try fileManager.createDirectory(at: fileURL.deletingLastPathComponent(), withIntermediateDirectories: true)
        try file.content_data.write(to: fileURL, options: .atomic)
    }
}

private func workspaceVaultRoot(vaultUID: String) -> URL {
    let cwd = URL(fileURLWithPath: FileManager.default.currentDirectoryPath, isDirectory: true)
    return cwd
        .appendingPathComponent("vault_storage", isDirectory: true)
        .appendingPathComponent(vaultUID, isDirectory: true)
}

private func workspaceVaultFileURL(vaultRoot: URL, filePath: String) -> URL {
    filePath
        .split(separator: "/", omittingEmptySubsequences: true)
        .reduce(vaultRoot) { partial, segment in
            partial.appendingPathComponent(String(segment), isDirectory: false)
        }
}

private func requireVaultUID(_ raw: String?) throws -> String {
    let vaultUID = try requireNonEmpty(raw, field: "vault_uid")
    if vaultUID.contains("\u{0}") {
        throw Abort(.badRequest, reason: "vault_uid contains invalid null byte")
    }
    if vaultUID.contains("/") || vaultUID.contains("\\") {
        throw Abort(.badRequest, reason: "vault_uid cannot contain path separators")
    }
    if vaultUID == "." || vaultUID == ".." {
        throw Abort(.badRequest, reason: "vault_uid is invalid")
    }
    if vaultUID.count > 255 {
        throw Abort(.badRequest, reason: "vault_uid is too long")
    }
    return vaultUID
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

private struct VaultIDRow: Decodable {
    let id: Int64
}

private struct VaultInsertedChangeRow: Decodable {
    let id: Int64
    let changed_at_unix_ms: Int64
}

private struct VaultLatestChangeRow: Decodable {
    let id: Int64
    let changed_at_unix_ms: Int64
}

private struct VaultPullChangeRow: Decodable {
    let change_id: Int64
    let file_path: String
    let action: String
    let changed_at_unix_ms: Int64
    let device_id: String?
    let content_base64: String?
    let content_sha256: String?
    let size_bytes: Int64?
}

private struct VaultStatusChangeRow: Decodable {
    let change_id: Int64
    let file_path: String
    let action: String
    let changed_at_unix_ms: Int64
    let device_id: String?
}

private struct VaultSnapshotRow: Decodable {
    let file_path: String
    let content_base64: String
    let content_sha256: String
    let size_bytes: Int64
    let updated_unix_ms: Int64
}

private struct VaultFileTimestampRow: Decodable {
    let file_path: String
    let updated_unix_ms: Int64
    let size_bytes: Int64
    let is_deleted: Bool
    let last_change_id: Int64?
}

private struct VaultFilePathRow: Decodable {
    let file_path: String
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
