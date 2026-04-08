import Crypto
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
          content TEXT NOT NULL,
          vector VECTOR(\(literal: config.embeddingDimension)) NOT NULL,
          type TEXT NOT NULL DEFAULT 'usercreated',
          parent TEXT DEFAULT NULL,
          metadata jsonb,
          created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
          CHECK (type IN ('usercreated', 'aicreated', 'imported'))
        );
        """
    ).run().wait()
    try sql.raw("\(unsafeRaw: legacyAtomsMigrationSQL(table: table))").run().wait()

    try sql.raw("CREATE INDEX IF NOT EXISTS \(ident: index) ON \(ident: table) USING hnsw (vector vector_cosine_ops);").run().wait()

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
        CREATE TABLE IF NOT EXISTS settings_sessions (
          id BIGSERIAL PRIMARY KEY,
          token_hash TEXT NOT NULL UNIQUE,
          created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
          last_seen_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
          expires_at TIMESTAMPTZ NOT NULL
        );
        """
    ).run().wait()
    try sql.raw("CREATE INDEX IF NOT EXISTS settings_sessions_expires_idx ON settings_sessions (expires_at);").run().wait()

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
          file_id BIGINT,
          device_id TEXT,
          file_path TEXT NOT NULL,
          action TEXT NOT NULL,
          changed_at_unix_ms BIGINT NOT NULL,
          content_base64 TEXT,
          content_sha256 TEXT,
          size_bytes BIGINT,
          received_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
          CHECK (action IN ('added', 'modified', 'deleted'))
        );
        """
    ).run().wait()
    try sql.raw("\(unsafeRaw: legacyVaultChangesMigrationSQL())").run().wait()
    try sql.raw("CREATE INDEX IF NOT EXISTS vault_changes_vault_time_idx ON vault_changes (vault_id, changed_at_unix_ms, id);").run().wait()
    try sql.raw("CREATE INDEX IF NOT EXISTS vault_changes_vault_id_idx ON vault_changes (vault_id, id);").run().wait()

    try sql.raw(
        """
        CREATE TABLE IF NOT EXISTS vault_files (
          id BIGSERIAL PRIMARY KEY,
          vault_id BIGINT NOT NULL REFERENCES vaults(id) ON DELETE CASCADE,
          file_path TEXT NOT NULL,
          name TEXT NOT NULL,
          base64 TEXT NOT NULL DEFAULT '',
          content TEXT,
          interpreted TEXT,
          subject TEXT,
          vector_subject VECTOR(\(literal: config.embeddingDimension)),
          vector_content VECTOR(\(literal: config.embeddingDimension)),
          content_sha256 TEXT NOT NULL DEFAULT '',
          size_bytes BIGINT NOT NULL DEFAULT 0,
          is_deleted BOOLEAN NOT NULL DEFAULT FALSE,
          updated_unix_ms BIGINT NOT NULL,
          content_version BIGINT NOT NULL DEFAULT 1,
          created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
          modified_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
          updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
          last_change_id BIGINT REFERENCES vault_changes(id) ON DELETE SET NULL,
          UNIQUE (vault_id, file_path)
        );
        """
    ).run().wait()
    try sql.raw("\(unsafeRaw: legacyVaultFilesMigrationSQL(dimension: config.embeddingDimension))").run().wait()
    try sql.raw("CREATE INDEX IF NOT EXISTS vault_files_vault_updated_idx ON vault_files (vault_id, updated_unix_ms DESC);").run().wait()
    try sql.raw("CREATE INDEX IF NOT EXISTS vault_files_vault_live_idx ON vault_files (vault_id, is_deleted, file_path);").run().wait()
    try sql.raw("CREATE INDEX IF NOT EXISTS vault_files_vector_content_hnsw ON vault_files USING hnsw (vector_content vector_cosine_ops);").run().wait()
    try sql.raw(
        """
        DO $$
        BEGIN
          IF NOT EXISTS (
            SELECT 1
            FROM pg_constraint
            WHERE conname = 'vault_changes_file_id_fkey'
          ) THEN
            ALTER TABLE vault_changes
            ADD CONSTRAINT vault_changes_file_id_fkey
            FOREIGN KEY (file_id) REFERENCES vault_files(id) ON DELETE SET NULL;
          END IF;
        END $$;
        """
    ).run().wait()

    try sql.raw(
        """
        CREATE TABLE IF NOT EXISTS file_atoms (
          file_id BIGINT NOT NULL REFERENCES vault_files(id) ON DELETE CASCADE,
          atom_id BIGINT NOT NULL,
          created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
          PRIMARY KEY (file_id, atom_id)
        );
        """
    ).run().wait()
    try sql.raw("CREATE INDEX IF NOT EXISTS file_atoms_atom_idx ON file_atoms (atom_id);").run().wait()

    try sql.raw(
        """
        CREATE TABLE IF NOT EXISTS file_links (
          id BIGSERIAL PRIMARY KEY,
          file_a_id BIGINT NOT NULL REFERENCES vault_files(id) ON DELETE CASCADE,
          file_b_id BIGINT NOT NULL REFERENCES vault_files(id) ON DELETE CASCADE,
          subject TEXT,
          absolute DOUBLE PRECISION,
          atoms BIGINT[] DEFAULT ARRAY[]::BIGINT[],
          content TEXT,
          created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
          updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
          UNIQUE (file_a_id, file_b_id),
          CHECK (file_a_id <> file_b_id)
        );
        """
    ).run().wait()
    try sql.raw("CREATE INDEX IF NOT EXISTS file_links_a_idx ON file_links (file_a_id);").run().wait()
    try sql.raw("CREATE INDEX IF NOT EXISTS file_links_b_idx ON file_links (file_b_id);").run().wait()

    try sql.raw(
        """
        CREATE TABLE IF NOT EXISTS file_processing_jobs (
          id BIGSERIAL PRIMARY KEY,
          vault_id BIGINT NOT NULL REFERENCES vaults(id) ON DELETE CASCADE,
          file_id BIGINT NOT NULL REFERENCES vault_files(id) ON DELETE CASCADE,
          job_type TEXT NOT NULL,
          status TEXT NOT NULL DEFAULT 'pending',
          file_version BIGINT NOT NULL,
          attempts INTEGER NOT NULL DEFAULT 0,
          locked_by TEXT,
          available_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
          started_at TIMESTAMPTZ,
          finished_at TIMESTAMPTZ,
          last_error TEXT,
          created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
          updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
          CHECK (job_type IN ('file_enrichment', 'atomize')),
          CHECK (status IN ('pending', 'running', 'completed', 'failed', 'superseded')),
          UNIQUE (file_id, job_type, file_version)
        );
        """
    ).run().wait()
    try sql.raw("CREATE INDEX IF NOT EXISTS file_processing_jobs_pending_idx ON file_processing_jobs (status, job_type, available_at, id);").run().wait()
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
        let atomsTable = req.application.context.config.embeddingsTable

        let txResult = try await req.db.transaction { tx async throws -> VaultSyncMutationResult in
            let txSQL = try sqlDatabase(from: tx)
            let vaultID = try await ensureVaultID(vaultUID: vaultUID, sql: txSQL)
            var latestInTx: VaultInsertedChangeRow?
            var shouldPruneGeneratedAtoms = false
            var affectedFileIDs = Set<Int64>()

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
                            content_base64,
                            content_sha256,
                            size_bytes
                        ) VALUES (
                            \(bind: vaultID),
                            NULLIF(\(bind: deviceID), ''),
                            \(bind: change.file_path),
                            \(bind: change.action.rawValue),
                            \(bind: change.changed_at_unix_ms),
                            \(bind: change.content_base64),
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
                let fileState = try await upsertVaultFile(
                    vaultID: vaultID,
                    filePath: change.file_path,
                    contentBase64: change.content_base64,
                    contentData: change.content_data,
                    contentSHA256: change.content_sha256,
                    sizeBytes: change.size_bytes ?? Int64(0),
                    isDeleted: change.action == .deleted,
                    updatedUnixMS: change.changed_at_unix_ms,
                    changeID: changeRow.id,
                    sql: txSQL
                )
                try await linkVaultChangeToFile(changeID: changeRow.id, fileID: fileState.id, sql: txSQL)
                try await clearFileAtomLinks(fileID: fileState.id, sql: txSQL)
                affectedFileIDs.insert(fileState.id)
                shouldPruneGeneratedAtoms = true

                if change.action == .deleted {
                    try await supersedeVaultFileJobs(fileID: fileState.id, sql: txSQL)
                } else {
                    try await enqueueVaultFileJobs(
                        fileID: fileState.id,
                        vaultID: vaultID,
                        fileVersion: fileState.content_version,
                        sql: txSQL
                    )
                }
            }

            if shouldPruneGeneratedAtoms {
                try await pruneOrphanedGeneratedAtoms(sql: txSQL, atomsTable: atomsTable)
            }

            guard let latestInTx else {
                throw Abort(.internalServerError, reason: "failed to resolve latest vault change")
            }
            return VaultSyncMutationResult(
                vault_id: vaultID,
                applied_changes: normalizedChanges.count,
                latest_change: latestInTx,
                affected_file_ids: affectedFileIDs
            )
        }

        do {
            try persistVaultChangesToWorkspace(vaultUID: vaultUID, changes: normalizedChanges)
        } catch {
            return VaultSyncPushResponse(
                ok: false,
                vault_uid: vaultUID,
                applied_changes: normalizedChanges.count,
                latest_change_id: txResult.latest_change?.id,
                latest_change_unix_ms: txResult.latest_change?.changed_at_unix_ms,
                error: "vault DB update succeeded, but writing workspace files failed: \(error.localizedDescription)"
            )
        }

        let settings = try await currentEmbeddingSettings(req: req)
        do {
            let processing = try await processVaultNotes(
                vaultID: txResult.vault_id,
                vaultUID: vaultUID,
                seedFileIDs: txResult.affected_file_ids,
                sql: try sqlDatabase(from: req.db),
                context: req.application.context,
                settings: settings
            )

            return VaultSyncPushResponse(
                ok: true,
                vault_uid: vaultUID,
                applied_changes: normalizedChanges.count,
                latest_change_id: processing.latestChangeID ?? txResult.latest_change?.id,
                latest_change_unix_ms: processing.latestChangeUnixMS ?? txResult.latest_change?.changed_at_unix_ms,
                error: nil
            )
        } catch {
            return VaultSyncPushResponse(
                ok: false,
                vault_uid: vaultUID,
                applied_changes: normalizedChanges.count,
                latest_change_id: txResult.latest_change?.id,
                latest_change_unix_ms: txResult.latest_change?.changed_at_unix_ms,
                error: "vault sync succeeded, but note processing failed: \(error.localizedDescription)"
            )
        }
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
                        content_base64,
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
                    base64 AS content_base64,
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
        let atomsTable = req.application.context.config.embeddingsTable

        let txResult = try await req.db.transaction { tx async throws -> VaultSyncMutationResult in
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
            var shouldPruneGeneratedAtoms = false
            var affectedFileIDs = Set<Int64>()

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
                            content_base64,
                            content_sha256,
                            size_bytes
                        ) VALUES (
                            \(bind: vaultID),
                            NULLIF(\(bind: deviceID), ''),
                            \(bind: file.file_path),
                            \(bind: action.rawValue),
                            \(bind: uploadedAtUnixMS),
                            \(bind: file.content_base64),
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
                let fileState = try await upsertVaultFile(
                    vaultID: vaultID,
                    filePath: file.file_path,
                    contentBase64: file.content_base64,
                    contentData: file.content_data,
                    contentSHA256: nil,
                    sizeBytes: file.size_bytes,
                    isDeleted: false,
                    updatedUnixMS: uploadedAtUnixMS,
                    changeID: changeRow.id,
                    sql: txSQL
                )
                try await linkVaultChangeToFile(changeID: changeRow.id, fileID: fileState.id, sql: txSQL)
                try await clearFileAtomLinks(fileID: fileState.id, sql: txSQL)
                affectedFileIDs.insert(fileState.id)
                shouldPruneGeneratedAtoms = true
                try await enqueueVaultFileJobs(
                    fileID: fileState.id,
                    vaultID: vaultID,
                    fileVersion: fileState.content_version,
                    sql: txSQL
                )
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
                            content_base64,
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
                let fileState = try await upsertVaultFile(
                    vaultID: vaultID,
                    filePath: filePath,
                    contentBase64: nil,
                    contentData: nil,
                    contentSHA256: nil,
                    sizeBytes: Int64(0),
                    isDeleted: true,
                    updatedUnixMS: uploadedAtUnixMS,
                    changeID: changeRow.id,
                    sql: txSQL
                )
                try await linkVaultChangeToFile(changeID: changeRow.id, fileID: fileState.id, sql: txSQL)
                try await clearFileAtomLinks(fileID: fileState.id, sql: txSQL)
                affectedFileIDs.insert(fileState.id)
                shouldPruneGeneratedAtoms = true
                try await supersedeVaultFileJobs(fileID: fileState.id, sql: txSQL)
            }

            if shouldPruneGeneratedAtoms {
                try await pruneOrphanedGeneratedAtoms(sql: txSQL, atomsTable: atomsTable)
            }

            return VaultSyncMutationResult(
                vault_id: vaultID,
                applied_changes: appliedChanges,
                latest_change: latestInTx,
                affected_file_ids: affectedFileIDs
            )
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

        let settings = try await currentEmbeddingSettings(req: req)
        do {
            let processing = try await processVaultNotes(
                vaultID: txResult.vault_id,
                vaultUID: vaultUID,
                seedFileIDs: txResult.affected_file_ids,
                sql: try sqlDatabase(from: req.db),
                context: req.application.context,
                settings: settings
            )

            return VaultSyncPushResponse(
                ok: true,
                vault_uid: vaultUID,
                applied_changes: txResult.applied_changes,
                latest_change_id: processing.latestChangeID ?? txResult.latest_change?.id,
                latest_change_unix_ms: processing.latestChangeUnixMS ?? txResult.latest_change?.changed_at_unix_ms,
                error: nil
            )
        } catch {
            return VaultSyncPushResponse(
                ok: false,
                vault_uid: vaultUID,
                applied_changes: txResult.applied_changes,
                latest_change_id: txResult.latest_change?.id,
                latest_change_unix_ms: txResult.latest_change?.changed_at_unix_ms,
                error: "vault sync succeeded, but note processing failed: \(error.localizedDescription)"
            )
        }
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
                    base64 AS content_base64,
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

    app.post("vaults", "search") { req async throws -> VaultSemanticSearchResponse in
        try await authorize(req)
        let payload = try req.content.decode(VaultSemanticSearchPayload.self)
        let vaultUID = try requireVaultUID(payload.vault_uid)
        let query = try requireText(payload.query)
        let limit = max(1, min(payload.limit ?? 10, 50))
        let settings = try await currentEmbeddingSettings(req: req)

        do {
            let results = try await semanticSearchVaultNotes(
                vaultUID: vaultUID,
                query: query,
                limit: limit,
                sql: try sqlDatabase(from: req.db),
                context: req.application.context,
                settings: settings
            )

            return VaultSemanticSearchResponse(
                ok: true,
                vault_uid: vaultUID,
                query: query,
                results: results,
                error: nil
            )
        } catch {
            return VaultSemanticSearchResponse(
                ok: false,
                vault_uid: vaultUID,
                query: query,
                results: nil,
                error: error.localizedDescription
            )
        }
    }

    app.get("settings", "login") { req async throws -> Response in
        let sql = try sqlDatabase(from: req.db)

        if let queryAPIKey = nonEmpty(req.query[String.self, at: "api_key"]) {
            do {
                let sessionToken = try await createSettingsSession(
                    apiKey: queryAPIKey,
                    sql: sql,
                    keyService: req.application.context.keyService,
                    secret: req.application.context.config.initialMasterKey
                )
                return redirectHTMLResponse(
                    location: "/settings",
                    setCookie: settingsSessionCookieHeader(token: sessionToken)
                )
            } catch {
                let body = renderSettingsLoginHTML(
                    errorMessage: "Invalid API key. Use a master key or a generated Foundation API key.",
                    signedOut: false,
                    config: req.application.context.config
                )
                return htmlResponse(body, status: .unauthorized, setCookie: clearSettingsSessionCookieHeader())
            }
        }

        if let authorization = try await resolveSettingsAuthorization(req: req, sql: sql) {
            if case .browserSession(let sessionToken) = authorization.mode {
                return redirectHTMLResponse(
                    location: "/settings",
                    setCookie: settingsSessionCookieHeader(token: sessionToken)
                )
            }
            return redirectHTMLResponse(location: "/settings")
        }

        let signedOut = req.query[String.self, at: "signed_out"] == "1"
        let body = renderSettingsLoginHTML(
            errorMessage: nil,
            signedOut: signedOut,
            config: req.application.context.config
        )
        return htmlResponse(body)
    }

    app.post("settings", "login") { req async throws -> Response in
        let form = try req.content.decode(SettingsLoginForm.self)
        let sql = try sqlDatabase(from: req.db)

        do {
            let sessionToken = try await createSettingsSession(
                apiKey: form.api_key,
                sql: sql,
                keyService: req.application.context.keyService,
                secret: req.application.context.config.initialMasterKey
            )
            return redirectHTMLResponse(
                location: "/settings",
                setCookie: settingsSessionCookieHeader(token: sessionToken)
            )
        } catch {
            let body = renderSettingsLoginHTML(
                errorMessage: "Invalid API key. Use a master key or a generated Foundation API key.",
                signedOut: false,
                config: req.application.context.config
            )
            return htmlResponse(body, status: .unauthorized)
        }
    }

    app.post("settings", "logout") { req async throws -> Response in
        if let token = settingsSessionToken(from: req) {
            let sql = try sqlDatabase(from: req.db)
            try await deleteSettingsSession(
                token: token,
                sql: sql,
                secret: req.application.context.config.initialMasterKey
            )
        }

        return redirectHTMLResponse(
            location: "/settings/login?signed_out=1",
            setCookie: clearSettingsSessionCookieHeader()
        )
    }

    app.get("settings") { req async throws -> Response in
        let sql = try sqlDatabase(from: req.db)

        if let queryAPIKey = nonEmpty(req.query[String.self, at: "api_key"]) {
            do {
                let sessionToken = try await createSettingsSession(
                    apiKey: queryAPIKey,
                    sql: sql,
                    keyService: req.application.context.keyService,
                    secret: req.application.context.config.initialMasterKey
                )
                return redirectHTMLResponse(
                    location: "/settings",
                    setCookie: settingsSessionCookieHeader(token: sessionToken)
                )
            } catch {
                let body = renderSettingsLoginHTML(
                    errorMessage: "Invalid API key. Use a master key or a generated Foundation API key.",
                    signedOut: false,
                    config: req.application.context.config
                )
                return htmlResponse(body, status: .unauthorized, setCookie: clearSettingsSessionCookieHeader())
            }
        }

        guard let authorization = try await resolveSettingsAuthorization(req: req, sql: sql) else {
            return redirectHTMLResponse(
                location: "/settings/login",
                setCookie: clearSettingsSessionCookieHeader()
            )
        }

        let settings = try await currentEmbeddingSettings(req: req)
        let body = renderSettingsHTML(
            settings: settings,
            saved: false,
            authorization: authorization,
            config: req.application.context.config
        )
        return htmlResponse(body)
    }

    app.post("settings") { req async throws -> Response in
        let form = try req.content.decode(SettingsForm.self)

        let authorization: SettingsAuthorization
        do {
            authorization = try await authorizeSettingsWrite(req, formAPIKey: form.api_key)
        } catch {
            return redirectHTMLResponse(
                location: "/settings/login",
                setCookie: clearSettingsSessionCookieHeader()
            )
        }

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
        let body = renderSettingsHTML(
            settings: updatedSettings,
            saved: true,
            authorization: authorization,
            config: req.application.context.config
        )
        return htmlResponse(body)
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
                .raw("SELECT id FROM \(ident: table) WHERE content = \(bind: text) LIMIT 1")
                .all()

            if !existing.isEmpty {
                return StandardResultResponse(ok: false, result: nil, error: "Text already exists")
            }

            try await sql
                .raw(
                    """
                    INSERT INTO \(ident: table) (content, vector, type)
                    VALUES (\(bind: text), (\(bind: vectorLiteral))::vector, \(bind: "usercreated"))
                    """
                )
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
                .raw("DELETE FROM \(ident: table) WHERE content = \(bind: text)")
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
                        content AS text,
                        metadata::text AS metadata,
                        (vector <-> (\(bind: vectorLiteral))::vector)::double precision AS distance
                    FROM \(ident: table)
                    ORDER BY vector <-> (\(bind: vectorLiteral))::vector
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

private func legacyAtomsMigrationSQL(table: String) -> String {
    """
    DO $$
    BEGIN
      IF EXISTS (
        SELECT 1
        FROM information_schema.columns
        WHERE table_schema = 'public' AND table_name = '\(table)' AND column_name = 'text'
      ) AND NOT EXISTS (
        SELECT 1
        FROM information_schema.columns
        WHERE table_schema = 'public' AND table_name = '\(table)' AND column_name = 'content'
      ) THEN
        EXECUTE 'ALTER TABLE public.\(table) RENAME COLUMN text TO content';
      END IF;

      IF EXISTS (
        SELECT 1
        FROM information_schema.columns
        WHERE table_schema = 'public' AND table_name = '\(table)' AND column_name = 'embedding'
      ) AND NOT EXISTS (
        SELECT 1
        FROM information_schema.columns
        WHERE table_schema = 'public' AND table_name = '\(table)' AND column_name = 'vector'
      ) THEN
        EXECUTE 'ALTER TABLE public.\(table) RENAME COLUMN embedding TO vector';
      END IF;

      IF NOT EXISTS (
        SELECT 1
        FROM information_schema.columns
        WHERE table_schema = 'public' AND table_name = '\(table)' AND column_name = 'type'
      ) THEN
        EXECUTE 'ALTER TABLE public.\(table) ADD COLUMN type TEXT NOT NULL DEFAULT ''usercreated''';
      END IF;
    END $$;
    """
}

private func legacyVaultChangesMigrationSQL() -> String {
    """
    DO $$
    BEGIN
      IF NOT EXISTS (
        SELECT 1
        FROM information_schema.columns
        WHERE table_schema = 'public' AND table_name = 'vault_changes' AND column_name = 'file_id'
      ) THEN
        ALTER TABLE vault_changes ADD COLUMN file_id BIGINT;
      END IF;

      IF NOT EXISTS (
        SELECT 1
        FROM information_schema.columns
        WHERE table_schema = 'public' AND table_name = 'vault_changes' AND column_name = 'content_base64'
      ) THEN
        ALTER TABLE vault_changes ADD COLUMN content_base64 TEXT;
      END IF;

      IF EXISTS (
        SELECT 1
        FROM information_schema.columns
        WHERE table_schema = 'public' AND table_name = 'vault_changes' AND column_name = 'content'
      ) THEN
        UPDATE vault_changes
        SET content_base64 = encode(content, 'base64')
        WHERE content IS NOT NULL
          AND COALESCE(content_base64, '') = '';
      END IF;
    END $$;
    """
}

private func legacyVaultFilesMigrationSQL(dimension: Int) -> String {
    """
    DO $$
    BEGIN
      IF EXISTS (
        SELECT 1
        FROM information_schema.columns
        WHERE table_schema = 'public' AND table_name = 'vault_files' AND column_name = 'content' AND udt_name = 'bytea'
      ) THEN
        ALTER TABLE vault_files RENAME COLUMN content TO content_bytes;
      END IF;

      IF NOT EXISTS (
        SELECT 1
        FROM information_schema.columns
        WHERE table_schema = 'public' AND table_name = 'vault_files' AND column_name = 'name'
      ) THEN
        ALTER TABLE vault_files ADD COLUMN name TEXT;
      END IF;

      IF NOT EXISTS (
        SELECT 1
        FROM information_schema.columns
        WHERE table_schema = 'public' AND table_name = 'vault_files' AND column_name = 'base64'
      ) THEN
        ALTER TABLE vault_files ADD COLUMN base64 TEXT NOT NULL DEFAULT '';
      END IF;

      IF NOT EXISTS (
        SELECT 1
        FROM information_schema.columns
        WHERE table_schema = 'public' AND table_name = 'vault_files' AND column_name = 'content'
      ) THEN
        ALTER TABLE vault_files ADD COLUMN content TEXT;
      END IF;

      IF NOT EXISTS (
        SELECT 1
        FROM information_schema.columns
        WHERE table_schema = 'public' AND table_name = 'vault_files' AND column_name = 'interpreted'
      ) THEN
        ALTER TABLE vault_files ADD COLUMN interpreted TEXT;
      END IF;

      IF NOT EXISTS (
        SELECT 1
        FROM information_schema.columns
        WHERE table_schema = 'public' AND table_name = 'vault_files' AND column_name = 'subject'
      ) THEN
        ALTER TABLE vault_files ADD COLUMN subject TEXT;
      END IF;

      IF NOT EXISTS (
        SELECT 1
        FROM information_schema.columns
        WHERE table_schema = 'public' AND table_name = 'vault_files' AND column_name = 'vector_subject'
      ) THEN
        ALTER TABLE vault_files ADD COLUMN vector_subject VECTOR(\(dimension));
      END IF;

      IF NOT EXISTS (
        SELECT 1
        FROM information_schema.columns
        WHERE table_schema = 'public' AND table_name = 'vault_files' AND column_name = 'vector_content'
      ) THEN
        ALTER TABLE vault_files ADD COLUMN vector_content VECTOR(\(dimension));
      END IF;

      IF NOT EXISTS (
        SELECT 1
        FROM information_schema.columns
        WHERE table_schema = 'public' AND table_name = 'vault_files' AND column_name = 'content_version'
      ) THEN
        ALTER TABLE vault_files ADD COLUMN content_version BIGINT NOT NULL DEFAULT 1;
      END IF;

      IF NOT EXISTS (
        SELECT 1
        FROM information_schema.columns
        WHERE table_schema = 'public' AND table_name = 'vault_files' AND column_name = 'created_at'
      ) THEN
        ALTER TABLE vault_files ADD COLUMN created_at TIMESTAMPTZ NOT NULL DEFAULT NOW();
      END IF;

      IF NOT EXISTS (
        SELECT 1
        FROM information_schema.columns
        WHERE table_schema = 'public' AND table_name = 'vault_files' AND column_name = 'modified_at'
      ) THEN
        ALTER TABLE vault_files ADD COLUMN modified_at TIMESTAMPTZ NOT NULL DEFAULT NOW();
      END IF;

      IF EXISTS (
        SELECT 1
        FROM information_schema.columns
        WHERE table_schema = 'public' AND table_name = 'vault_files' AND column_name = 'content_bytes'
      ) THEN
        UPDATE vault_files
        SET base64 = encode(content_bytes, 'base64')
        WHERE content_bytes IS NOT NULL
          AND COALESCE(base64, '') = '';
      END IF;

      UPDATE vault_files
      SET name = file_path
      WHERE name IS NULL OR name = '';

      ALTER TABLE vault_files ALTER COLUMN name SET NOT NULL;
    END $$;
    """
}

private let settingsSessionCookieName = "foundation_settings_session"
private let settingsSessionLifetimeSeconds = 60 * 60 * 24 * 7

private enum SettingsAuthorizationMode {
    case apiKey
    case browserSession(token: String)
}

private struct SettingsAuthorization {
    let mode: SettingsAuthorizationMode

    var authLabel: String {
        switch mode {
        case .apiKey:
            return "Bearer API key"
        case .browserSession:
            return "Browser session"
        }
    }

    var authDescription: String {
        switch mode {
        case .apiKey:
            return "Authenticated directly from the request header. Useful for scripts and local API clients."
        case .browserSession:
            return "Authenticated through a secure browser session cookie backed by the server database."
        }
    }

    var showsLogout: Bool {
        if case .browserSession = mode {
            return true
        }
        return false
    }
}

private func authorizeSettingsWrite(_ req: Request, formAPIKey: String?) async throws -> SettingsAuthorization {
    let sql = try sqlDatabase(from: req.db)

    if let authorization = try await resolveSettingsAuthorization(req: req, sql: sql) {
        return authorization
    }

    if let formAPIKey = nonEmpty(formAPIKey) {
        let valid = try await req.application.context.keyService.verify(apiKey: formAPIKey, db: sql)
        guard valid else {
            throw Abort(.unauthorized, reason: "Invalid API Key")
        }
        return SettingsAuthorization(mode: .apiKey)
    }

    throw Abort(.unauthorized, reason: "Invalid API Key")
}

private func resolveSettingsAuthorization(req: Request, sql: any SQLDatabase) async throws -> SettingsAuthorization? {
    if let bearerToken = req.headers.bearerAuthorization?.token {
        let valid = try await req.application.context.keyService.verify(apiKey: bearerToken, db: sql)
        if valid {
            return SettingsAuthorization(mode: .apiKey)
        }
    }

    if let sessionToken = settingsSessionToken(from: req) {
        let valid = try await validateSettingsSession(
            token: sessionToken,
            sql: sql,
            secret: req.application.context.config.initialMasterKey
        )
        if valid {
            return SettingsAuthorization(mode: .browserSession(token: sessionToken))
        }
    }

    return nil
}

private func createSettingsSession(
    apiKey: String,
    sql: any SQLDatabase,
    keyService: KeyService,
    secret: String
) async throws -> String {
    try await cleanupExpiredSettingsSessions(sql: sql)

    let normalizedAPIKey = apiKey.trimmingCharacters(in: .whitespacesAndNewlines)
    guard !normalizedAPIKey.isEmpty else {
        throw Abort(.unauthorized, reason: "Invalid API Key")
    }

    let valid = try await keyService.verify(apiKey: normalizedAPIKey, db: sql)
    guard valid else {
        throw Abort(.unauthorized, reason: "Invalid API Key")
    }

    let token = "fst_" + randomSettingsSessionToken(length: 48)
    let tokenHash = settingsSessionTokenHash(token: token, secret: secret)

    try await sql
        .raw(
            """
            INSERT INTO settings_sessions (token_hash, expires_at)
            VALUES (
              \(bind: tokenHash),
              NOW() + make_interval(secs => \(literal: settingsSessionLifetimeSeconds))
            )
            """
        )
        .run()

    return token
}

private func validateSettingsSession(token: String, sql: any SQLDatabase, secret: String) async throws -> Bool {
    try await cleanupExpiredSettingsSessions(sql: sql)

    let tokenHash = settingsSessionTokenHash(token: token, secret: secret)
    let rows = try await sql
        .raw(
            """
            SELECT id
            FROM settings_sessions
            WHERE token_hash = \(bind: tokenHash)
              AND expires_at > NOW()
            LIMIT 1
            """
        )
        .all(decoding: SessionIdentityRow.self)

    guard !rows.isEmpty else {
        return false
    }

    try await sql
        .raw(
            """
            UPDATE settings_sessions
            SET last_seen_at = NOW()
            WHERE token_hash = \(bind: tokenHash)
            """
        )
        .run()

    return true
}

private func deleteSettingsSession(token: String, sql: any SQLDatabase, secret: String) async throws {
    try await cleanupExpiredSettingsSessions(sql: sql)
    let tokenHash = settingsSessionTokenHash(token: token, secret: secret)
    try await sql
        .raw("DELETE FROM settings_sessions WHERE token_hash = \(bind: tokenHash)")
        .run()
}

private func cleanupExpiredSettingsSessions(sql: any SQLDatabase) async throws {
    try await sql
        .raw("DELETE FROM settings_sessions WHERE expires_at <= NOW()")
        .run()
}

private func settingsSessionToken(from req: Request) -> String? {
    nonEmpty(req.cookies[settingsSessionCookieName]?.string)
}

private func settingsSessionTokenHash(token: String, secret: String) -> String {
    let digest = SHA256.hash(data: Data((secret + ":" + token).utf8))
    return digest.map { String(format: "%02x", $0) }.joined()
}

private func randomSettingsSessionToken(length: Int) -> String {
    let charset = Array("abcdefghijklmnopqrstuvwxyzABCDEFGHIJKLMNOPQRSTUVWXYZ0123456789")
    var generator = SystemRandomNumberGenerator()
    return String((0..<length).map { _ in charset.randomElement(using: &generator)! })
}

private func settingsSessionCookieHeader(token: String) -> String {
    "\(settingsSessionCookieName)=\(token); Max-Age=\(settingsSessionLifetimeSeconds); Path=/settings; HttpOnly; SameSite=Lax"
}

private func clearSettingsSessionCookieHeader() -> String {
    "\(settingsSessionCookieName)=; Max-Age=0; Path=/settings; HttpOnly; SameSite=Lax"
}

private func htmlResponse(_ body: String, status: HTTPResponseStatus = .ok, setCookie: String? = nil) -> Response {
    var headers = HTTPHeaders()
    headers.contentType = .html
    if let setCookie {
        headers.add(name: .setCookie, value: setCookie)
    }
    return Response(status: status, headers: headers, body: .init(string: body))
}

private func redirectHTMLResponse(location: String, setCookie: String? = nil) -> Response {
    var headers = HTTPHeaders()
    headers.replaceOrAdd(name: .location, value: location)
    if let setCookie {
        headers.add(name: .setCookie, value: setCookie)
    }
    return Response(status: .seeOther, headers: headers)
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
        .raw("SELECT id FROM \(ident: atomsTable) WHERE content = \(bind: atomText) LIMIT 1")
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
                AVG(a.vector) AS embedding,
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

enum VaultChangeAction: String {
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

private enum FileProcessingJobType: String, CaseIterable {
    case fileEnrichment = "file_enrichment"
    case atomize
}

private struct NormalizedVaultChange {
    let file_path: String
    let action: VaultChangeAction
    let changed_at_unix_ms: Int64
    let content_base64: String?
    let content_data: Data?
    let content_sha256: String?
    let size_bytes: Int64?
}

private struct NormalizedVaultFullFile {
    let file_path: String
    let content_base64: String
    let content_data: Data
    let size_bytes: Int64
}

private struct VaultSyncMutationResult {
    let vault_id: Int64
    let applied_changes: Int
    let latest_change: VaultInsertedChangeRow?
    let affected_file_ids: Set<Int64>
}

private struct VaultFileStateRow: Decodable {
    let id: Int64
    let content_version: Int64
}

func unixMillisecondsNow() -> Int64 {
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
            content_base64: nil,
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
        content_base64: contentData.base64EncodedString(),
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
                content_base64: contentData.base64EncodedString(),
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

private func decodeTextContent(from data: Data) -> String? {
    if data.isEmpty {
        return ""
    }
    guard let text = String(data: data, encoding: .utf8) else {
        return nil
    }

    let scalars = Array(text.unicodeScalars)
    guard !scalars.isEmpty else {
        return ""
    }

    let disallowedControlCount = scalars.reduce(into: 0) { count, scalar in
        if CharacterSet.controlCharacters.contains(scalar),
           scalar.value != 9,
           scalar.value != 10,
           scalar.value != 13 {
            count += 1
        }
    }

    let ratio = Double(disallowedControlCount) / Double(scalars.count)
    return ratio <= 0.05 ? text : nil
}

private func upsertVaultFile(
    vaultID: Int64,
    filePath: String,
    contentBase64: String?,
    contentData: Data?,
    contentSHA256: String?,
    sizeBytes: Int64,
    isDeleted: Bool,
    updatedUnixMS: Int64,
    changeID: Int64,
    sql: any SQLDatabase
) async throws -> VaultFileStateRow {
    let contentText = contentData.flatMap { decodeTextContent(from: $0) }
    let storedContent: String? = isDeleted ? nil : contentText
    let base64 = isDeleted ? "" : (contentBase64 ?? "")
    let checksum = isDeleted ? "" : (contentSHA256 ?? "")

    let rows = try await sql
        .raw(
            """
            INSERT INTO vault_files (
                vault_id,
                file_path,
                name,
                base64,
                content,
                interpreted,
                subject,
                vector_subject,
                vector_content,
                content_sha256,
                size_bytes,
                is_deleted,
                updated_unix_ms,
                content_version,
                created_at,
                modified_at,
                updated_at,
                last_change_id
            ) VALUES (
                \(bind: vaultID),
                \(bind: filePath),
                \(bind: filePath),
                \(bind: base64),
                \(bind: storedContent),
                NULL,
                NULL,
                NULL,
                NULL,
                \(bind: checksum),
                \(bind: sizeBytes),
                \(bind: isDeleted),
                \(bind: updatedUnixMS),
                1,
                NOW(),
                NOW(),
                NOW(),
                \(bind: changeID)
            )
            ON CONFLICT (vault_id, file_path)
            DO UPDATE SET
                name = EXCLUDED.name,
                base64 = EXCLUDED.base64,
                content = EXCLUDED.content,
                interpreted = NULL,
                subject = NULL,
                vector_subject = NULL,
                vector_content = NULL,
                content_sha256 = EXCLUDED.content_sha256,
                size_bytes = EXCLUDED.size_bytes,
                is_deleted = EXCLUDED.is_deleted,
                updated_unix_ms = EXCLUDED.updated_unix_ms,
                content_version = vault_files.content_version + 1,
                modified_at = NOW(),
                updated_at = NOW(),
                last_change_id = EXCLUDED.last_change_id
            RETURNING id, content_version
            """
        )
        .all(decoding: VaultFileStateRow.self)

    guard let row = rows.first else {
        throw Abort(.internalServerError, reason: "failed to upsert vault file")
    }
    return row
}

private func linkVaultChangeToFile(changeID: Int64, fileID: Int64, sql: any SQLDatabase) async throws {
    try await sql
        .raw(
            """
            UPDATE vault_changes
            SET file_id = \(bind: fileID)
            WHERE id = \(bind: changeID)
            """
        )
        .run()
}

private func supersedeVaultFileJobs(fileID: Int64, sql: any SQLDatabase) async throws {
    try await sql
        .raw(
            """
            UPDATE file_processing_jobs
            SET status = 'superseded',
                finished_at = COALESCE(finished_at, NOW()),
                updated_at = NOW()
            WHERE file_id = \(bind: fileID)
              AND status IN ('pending', 'running')
            """
        )
        .run()
}

private func enqueueVaultFileJobs(fileID: Int64, vaultID: Int64, fileVersion: Int64, sql: any SQLDatabase) async throws {
    try await supersedeVaultFileJobs(fileID: fileID, sql: sql)
    try await sql
        .raw(
            """
            INSERT INTO file_processing_jobs (vault_id, file_id, job_type, status, file_version)
            VALUES
                (\(bind: vaultID), \(bind: fileID), \(bind: FileProcessingJobType.fileEnrichment.rawValue), 'pending', \(bind: fileVersion)),
                (\(bind: vaultID), \(bind: fileID), \(bind: FileProcessingJobType.atomize.rawValue), 'pending', \(bind: fileVersion))
            ON CONFLICT (file_id, job_type, file_version) DO NOTHING
            """
        )
        .run()
}

private func clearFileAtomLinks(fileID: Int64, sql: any SQLDatabase) async throws {
    try await sql
        .raw("DELETE FROM file_atoms WHERE file_id = \(bind: fileID)")
        .run()
}

private func pruneOrphanedGeneratedAtoms(sql: any SQLDatabase, atomsTable: String) async throws {
    try await sql
        .raw(
            """
            DELETE FROM \(ident: atomsTable) a
            WHERE a.type IN ('aicreated', 'imported')
              AND NOT EXISTS (
                SELECT 1
                FROM file_atoms fa
                WHERE fa.atom_id = a.id
              )
              AND NOT EXISTS (
                SELECT 1
                FROM source_atoms sa
                WHERE sa.atom_id = a.id
              )
            """
        )
        .run()
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

private func renderSettingsLoginHTML(errorMessage: String?, signedOut: Bool, config: AppConfig) -> String {
    let errorBanner = errorMessage.map {
        "<div class=\"flash flash-error\">" + escapeHTML($0) + "</div>"
    } ?? ""
    let signedOutBanner = signedOut ? "<div class=\"flash flash-success\">Signed out. Your browser session has been cleared.</div>" : ""

    let content = """
    <div class="site-shell">
      <header class="topbar">
        <div class="brand-lockup">
          <div class="brand-mark">F</div>
          <div>
            <div class="brand-title">Foundation Settings</div>
            <div class="brand-subtitle">Secure browser access for embedding configuration</div>
          </div>
        </div>
        <div class="topbar-note">Local admin console</div>
      </header>

      <section class="hero-panel">
        <div>
          <div class="eyebrow">Admin login</div>
          <h1>Sign in before changing server embedding behavior.</h1>
          <p class="hero-copy">This screen protects the provider, model, and OpenAI key used by <code>/embed/text</code>, <code>/add</code>, and <code>/find</code>.</p>
        </div>
        <div class="hero-stats">
          <div class="stat-card">
            <span class="stat-label">Embedding dimension</span>
            <strong>\(config.embeddingDimension)</strong>
          </div>
          <div class="stat-card">
            <span class="stat-label">Vector table</span>
            <strong>\(escapeHTML(config.embeddingsTable))</strong>
          </div>
          <div class="stat-card">
            <span class="stat-label">Default provider</span>
            <strong>\(escapeHTML(config.defaultEmbeddingProvider.displayName))</strong>
          </div>
        </div>
      </section>

      <section class="layout-grid login-grid">
        <div class="panel">
          <div class="section-head">
            <div>
              <div class="eyebrow">Authentication</div>
              <h2>Browser login</h2>
            </div>
          </div>
          \(errorBanner)
          \(signedOutBanner)
          <form method="post" action="/settings/login" class="stack-form">
            <label for="api_key">Foundation API key</label>
            <input id="api_key" name="api_key" type="password" placeholder="foundation_..." autocomplete="current-password" autofocus />
            <p class="field-note">Use the master key or any generated Foundation API key.</p>
            <button class="primary-button" type="submit">Continue to settings</button>
          </form>
        </div>

        <aside class="panel muted-panel">
          <div class="section-head">
            <div>
              <div class="eyebrow">What changes here</div>
              <h2>Control surface</h2>
            </div>
          </div>
          <div class="info-list">
            <div class="info-item">
              <strong>Provider selection</strong>
              <span>Switch between local deterministic Qwen mode and OpenAI embeddings.</span>
            </div>
            <div class="info-item">
              <strong>Model defaults</strong>
              <span>Update the model labels stored in <code>app_settings</code>.</span>
            </div>
            <div class="info-item">
              <strong>Secret management</strong>
              <span>Store or clear the OpenAI API key without exposing it back to the browser.</span>
            </div>
          </div>
        </aside>
      </section>
    </div>
    """

    return renderSettingsDocument(pageTitle: "Foundation Settings Login", content: content, script: "")
}

private func renderSettingsHTML(
    settings: EmbeddingSettings,
    saved: Bool,
    authorization: SettingsAuthorization,
    config: AppConfig
) -> String {
    var modelOptions = EmbeddingSettings.availableOpenAIModels
    if !modelOptions.contains(settings.openAIModel) {
        modelOptions.append(settings.openAIModel)
    }

    var optionFragments: [String] = []
    optionFragments.reserveCapacity(modelOptions.count)
    for model in modelOptions {
        let selected = model == settings.openAIModel ? " selected" : ""
        let fragment = "<option value=\"" + escapeHTML(model) + "\"" + selected + ">" + escapeHTML(model) + "</option>"
        optionFragments.append(fragment)
    }
    let optionsHTML = optionFragments.joined(separator: "")

    let qwenChecked = settings.provider == .qwen3 ? " checked" : ""
    let openAIChecked = settings.provider == .openai ? " checked" : ""
    let savedBanner = saved ? "<div class=\"flash flash-success\">Settings saved. New embedding requests will use the updated configuration.</div>" : ""
    let isAPIKeyAuthorization: Bool
    switch authorization.mode {
    case .apiKey:
        isAPIKeyAuthorization = true
    case .browserSession:
        isAPIKeyAuthorization = false
    }
    let authBanner = isAPIKeyAuthorization
        ? "<div class=\"flash flash-info\">This page was opened with Bearer auth. For a persistent browser session, use <code>/settings/login</code>.</div>"
        : ""
    let openAIKeyState = settings.openAIAPIKey == nil ? "Not stored" : "Stored"

    let logoutAction = authorization.showsLogout
        ? """
          <form method="post" action="/settings/logout">
            <button class="ghost-button" type="submit">Log out</button>
          </form>
          """
        : ""

    let content = """
    <div class="site-shell">
      <header class="topbar">
        <div class="brand-lockup">
          <div class="brand-mark">F</div>
          <div>
            <div class="brand-title">Foundation Settings</div>
            <div class="brand-subtitle">Embedding provider and secret management</div>
          </div>
        </div>
        <div class="topbar-actions">
          <span class="status-chip">\(escapeHTML(authorization.authLabel))</span>
          \(logoutAction)
        </div>
      </header>

      <section class="hero-panel">
        <div>
          <div class="eyebrow">Settings dashboard</div>
          <h1>Configure how the server builds embeddings.</h1>
          <p class="hero-copy">Changes here affect text embedding generation, similarity search, and any asynchronous enrichment jobs that rely on the current embedding backend.</p>
        </div>
        <div class="hero-stats">
          <div class="stat-card">
            <span class="stat-label">Current provider</span>
            <strong data-provider-summary>\(escapeHTML(settings.provider.displayName))</strong>
          </div>
          <div class="stat-card">
            <span class="stat-label">OpenAI key</span>
            <strong>\(escapeHTML(openAIKeyState))</strong>
          </div>
          <div class="stat-card">
            <span class="stat-label">Embedding dimension</span>
            <strong>\(config.embeddingDimension)</strong>
          </div>
        </div>
      </section>

      <section class="layout-grid">
        <aside class="panel muted-panel">
          <div class="section-head">
            <div>
              <div class="eyebrow">Session</div>
              <h2>Runtime status</h2>
            </div>
          </div>
          <dl class="meta-grid">
            <div>
              <dt>Authentication</dt>
              <dd>\(escapeHTML(authorization.authLabel))</dd>
            </div>
            <div>
              <dt>Access mode</dt>
              <dd>\(escapeHTML(authorization.authDescription))</dd>
            </div>
            <div>
              <dt>Atom table</dt>
              <dd>\(escapeHTML(config.embeddingsTable))</dd>
            </div>
            <div>
              <dt>OpenAI key</dt>
              <dd>\(escapeHTML(maskAPIKey(settings.openAIAPIKey)))</dd>
            </div>
          </dl>
          <div class="helper-note">OpenAI vectors are resized to the server dimension for compatibility. Qwen mode stays local and deterministic.</div>
        </aside>

        <div class="panel">
          <div class="section-head">
            <div>
              <div class="eyebrow">Configuration</div>
              <h2>Embedding settings</h2>
            </div>
          </div>
          \(savedBanner)
          \(authBanner)
          <form method="post" action="/settings" class="stack-form">
            <div class="provider-grid">
              <label class="provider-card" data-provider-card="qwen3">
                <input type="radio" name="provider" value="qwen3"\(qwenChecked) />
                <span class="provider-kicker">Local mode</span>
                <strong>\(escapeHTML(EmbeddingProvider.qwen3.displayName))</strong>
                <span>Fast deterministic embeddings with no remote API dependency.</span>
              </label>
              <label class="provider-card" data-provider-card="openai">
                <input type="radio" name="provider" value="openai"\(openAIChecked) />
                <span class="provider-kicker">Remote mode</span>
                <strong>\(escapeHTML(EmbeddingProvider.openai.displayName))</strong>
                <span>Uses OpenAI embeddings and keeps the API key stored server-side.</span>
              </label>
            </div>

            <div class="form-section" data-provider-group="qwen3">
              <label for="qwen_model">Qwen model label</label>
              <input id="qwen_model" name="qwen_model" value="\(escapeHTML(settings.qwenModel))" />
              <p class="field-note">This label is informational. Qwen mode still uses the local deterministic embedding backend.</p>
            </div>

            <div class="form-section" data-provider-group="openai">
              <label for="openai_model">OpenAI embedding model</label>
              <select id="openai_model" name="openai_model">\(optionsHTML)</select>

              <label for="openai_api_key">OpenAI API key</label>
              <input id="openai_api_key" name="openai_api_key" type="password" placeholder="\(escapeHTML(maskAPIKey(settings.openAIAPIKey)))" autocomplete="off" />
              <p class="field-note">Leave this empty to keep the stored key. Use the checkbox below to remove it completely.</p>

              <label class="checkbox-row">
                <input type="checkbox" name="clear_openai_key" value="1" />
                <span>Clear the stored OpenAI API key</span>
              </label>
            </div>

            <div class="form-actions">
              <button class="primary-button" type="submit">Save settings</button>
              <span class="helper-note">Applies to future embedding requests immediately.</span>
            </div>
          </form>
        </div>
      </section>
    </div>
    """

    let script = """
    const providerInputs = document.querySelectorAll('input[name="provider"]');
    const providerCards = document.querySelectorAll('[data-provider-card]');
    const providerGroups = document.querySelectorAll('[data-provider-group]');
    const providerSummary = document.querySelector('[data-provider-summary]');
    const labels = {
      qwen3: "\(escapeHTML(EmbeddingProvider.qwen3.displayName))",
      openai: "\(escapeHTML(EmbeddingProvider.openai.displayName))"
    };

    function syncProviderUI() {
      const selected = document.querySelector('input[name="provider"]:checked')?.value || "qwen3";
      providerCards.forEach((card) => {
        card.dataset.active = card.dataset.providerCard === selected ? "true" : "false";
      });
      providerGroups.forEach((group) => {
        group.hidden = group.dataset.providerGroup !== selected;
      });
      if (providerSummary) {
        providerSummary.textContent = labels[selected] || selected;
      }
    }

    providerInputs.forEach((input) => {
      input.addEventListener("change", syncProviderUI);
    });
    syncProviderUI();
    """

    return renderSettingsDocument(pageTitle: "Foundation Settings", content: content, script: script)
}

private func renderSettingsDocument(pageTitle: String, content: String, script: String) -> String {
    let scriptBlock = script.isEmpty ? "" : "<script>" + script + "</script>"

    return """
    <!doctype html>
    <html lang="en">
      <head>
        <meta charset="utf-8" />
        <meta name="viewport" content="width=device-width, initial-scale=1" />
        <title>\(escapeHTML(pageTitle))</title>
        <style>
          :root {
            --canvas: #08111f;
            --canvas-top: #143056;
            --panel: rgba(8, 17, 31, 0.78);
            --panel-soft: rgba(255, 255, 255, 0.08);
            --line: rgba(203, 221, 247, 0.18);
            --ink: #f2f5fb;
            --muted: #a8b6ce;
            --accent: #f38b4a;
            --accent-strong: #ff6b35;
            --success: #5ad1a6;
            --danger: #ff8d8d;
            --info: #7cc8ff;
          }
          * { box-sizing: border-box; }
          [hidden] { display: none !important; }
          body {
            margin: 0;
            min-height: 100vh;
            font-family: "Avenir Next", "Pretendard", "Noto Sans KR", "Segoe UI", sans-serif;
            color: var(--ink);
            background:
              radial-gradient(circle at top left, rgba(243, 139, 74, 0.22), transparent 26%),
              radial-gradient(circle at top right, rgba(124, 200, 255, 0.16), transparent 24%),
              linear-gradient(160deg, var(--canvas-top) 0%, var(--canvas) 48%, #050a13 100%);
          }
          body::before {
            content: "";
            position: fixed;
            inset: 0;
            pointer-events: none;
            background-image: linear-gradient(rgba(255, 255, 255, 0.04) 1px, transparent 1px), linear-gradient(90deg, rgba(255, 255, 255, 0.04) 1px, transparent 1px);
            background-size: 48px 48px;
            mask-image: radial-gradient(circle at center, black, transparent 80%);
          }
          code {
            padding: 0.1rem 0.35rem;
            border-radius: 999px;
            background: rgba(255, 255, 255, 0.08);
            font-family: "SFMono-Regular", "SF Mono", Menlo, monospace;
            font-size: 0.92em;
          }
          .site-shell {
            position: relative;
            max-width: 1180px;
            margin: 0 auto;
            padding: 28px 18px 42px;
          }
          .topbar,
          .topbar-actions,
          .brand-lockup,
          .hero-stats,
          .section-head,
          .form-actions,
          .checkbox-row {
            display: flex;
            align-items: center;
          }
          .topbar {
            justify-content: space-between;
            gap: 16px;
            margin-bottom: 20px;
          }
          .brand-lockup {
            gap: 14px;
          }
          .brand-mark {
            width: 44px;
            height: 44px;
            display: grid;
            place-items: center;
            border-radius: 14px;
            background: linear-gradient(135deg, var(--accent) 0%, var(--accent-strong) 100%);
            color: #09111d;
            font-weight: 800;
            font-size: 1.15rem;
            box-shadow: 0 18px 40px rgba(243, 139, 74, 0.22);
          }
          .brand-title {
            font-size: 1rem;
            font-weight: 800;
            letter-spacing: 0.01em;
          }
          .brand-subtitle,
          .topbar-note,
          .hero-copy,
          .field-note,
          .helper-note,
          .info-item span {
            color: var(--muted);
          }
          .topbar-actions {
            gap: 10px;
          }
          .status-chip,
          .eyebrow {
            display: inline-flex;
            align-items: center;
            gap: 8px;
            border-radius: 999px;
            padding: 0.45rem 0.8rem;
            background: rgba(255, 255, 255, 0.08);
            border: 1px solid rgba(255, 255, 255, 0.08);
          }
          .eyebrow {
            width: fit-content;
            margin-bottom: 10px;
            text-transform: uppercase;
            font-size: 0.72rem;
            letter-spacing: 0.15em;
            color: #d4deef;
          }
          h1, h2 {
            margin: 0;
            line-height: 1.05;
          }
          h1 {
            font-size: clamp(2.1rem, 4vw, 3.5rem);
            max-width: 12ch;
          }
          h2 {
            font-size: 1.4rem;
          }
          .hero-panel,
          .panel {
            position: relative;
            overflow: hidden;
            border: 1px solid var(--line);
            background: var(--panel);
            backdrop-filter: blur(14px);
            box-shadow: 0 30px 80px rgba(0, 0, 0, 0.24);
          }
          .hero-panel {
            border-radius: 28px;
            padding: 28px;
            display: grid;
            grid-template-columns: minmax(0, 1.4fr) minmax(280px, 0.8fr);
            gap: 20px;
            margin-bottom: 18px;
          }
          .hero-stats {
            justify-content: flex-end;
            gap: 12px;
            flex-wrap: wrap;
          }
          .stat-card,
          .panel {
            border-radius: 22px;
          }
          .stat-card {
            min-width: 150px;
            padding: 16px 18px;
            background: rgba(255, 255, 255, 0.08);
            border: 1px solid rgba(255, 255, 255, 0.08);
          }
          .stat-label,
          dt,
          .provider-kicker {
            display: block;
            margin-bottom: 6px;
            font-size: 0.78rem;
            letter-spacing: 0.08em;
            text-transform: uppercase;
            color: var(--muted);
          }
          .layout-grid {
            display: grid;
            grid-template-columns: minmax(280px, 340px) minmax(0, 1fr);
            gap: 18px;
          }
          .login-grid {
            grid-template-columns: minmax(0, 1.1fr) minmax(280px, 0.9fr);
          }
          .panel {
            padding: 24px;
          }
          .muted-panel {
            background: rgba(255, 255, 255, 0.06);
          }
          .section-head {
            justify-content: space-between;
            gap: 16px;
            margin-bottom: 18px;
          }
          .flash {
            margin-bottom: 14px;
            padding: 13px 14px;
            border-radius: 16px;
            border: 1px solid transparent;
            font-weight: 600;
          }
          .flash-success {
            color: #ddffef;
            background: rgba(90, 209, 166, 0.12);
            border-color: rgba(90, 209, 166, 0.32);
          }
          .flash-error {
            color: #ffe3e3;
            background: rgba(255, 141, 141, 0.14);
            border-color: rgba(255, 141, 141, 0.34);
          }
          .flash-info {
            color: #dff2ff;
            background: rgba(124, 200, 255, 0.12);
            border-color: rgba(124, 200, 255, 0.3);
          }
          .stack-form {
            display: grid;
            gap: 14px;
          }
          label {
            font-size: 0.92rem;
            font-weight: 700;
          }
          input,
          select,
          button {
            font: inherit;
          }
          input,
          select {
            width: 100%;
            border: 1px solid rgba(255, 255, 255, 0.12);
            border-radius: 16px;
            background: rgba(255, 255, 255, 0.06);
            color: var(--ink);
            padding: 0.95rem 1rem;
            outline: none;
          }
          input::placeholder {
            color: rgba(242, 245, 251, 0.42);
          }
          input:focus,
          select:focus {
            border-color: rgba(243, 139, 74, 0.7);
            box-shadow: 0 0 0 4px rgba(243, 139, 74, 0.16);
          }
          .provider-grid {
            display: grid;
            grid-template-columns: repeat(2, minmax(0, 1fr));
            gap: 12px;
          }
          .provider-card {
            display: block;
            padding: 18px;
            border-radius: 18px;
            border: 1px solid rgba(255, 255, 255, 0.12);
            background: rgba(255, 255, 255, 0.05);
            cursor: pointer;
            transition: transform 140ms ease, border-color 140ms ease, background 140ms ease;
          }
          .provider-card:hover {
            transform: translateY(-2px);
            border-color: rgba(243, 139, 74, 0.46);
          }
          .provider-card[data-active="true"] {
            border-color: rgba(243, 139, 74, 0.9);
            background: rgba(243, 139, 74, 0.14);
            box-shadow: inset 0 0 0 1px rgba(243, 139, 74, 0.16);
          }
          .provider-card input {
            position: absolute;
            opacity: 0;
            pointer-events: none;
          }
          .provider-card strong,
          .info-item strong {
            display: block;
            margin-bottom: 8px;
            font-size: 1rem;
          }
          .provider-card span {
            display: block;
            line-height: 1.45;
            color: var(--muted);
          }
          .form-section {
            padding-top: 8px;
            border-top: 1px solid rgba(255, 255, 255, 0.08);
          }
          .checkbox-row {
            gap: 10px;
            margin-top: 8px;
            font-weight: 600;
          }
          .checkbox-row input {
            width: 18px;
            height: 18px;
            padding: 0;
          }
          .form-actions {
            justify-content: space-between;
            gap: 14px;
            padding-top: 6px;
            flex-wrap: wrap;
          }
          .primary-button,
          .ghost-button {
            border: 0;
            border-radius: 999px;
            padding: 0.95rem 1.35rem;
            font-weight: 800;
            cursor: pointer;
            transition: transform 140ms ease, filter 140ms ease;
          }
          .primary-button {
            color: #09111d;
            background: linear-gradient(135deg, var(--accent) 0%, var(--accent-strong) 100%);
            box-shadow: 0 18px 40px rgba(243, 139, 74, 0.22);
          }
          .ghost-button {
            color: var(--ink);
            background: rgba(255, 255, 255, 0.08);
            border: 1px solid rgba(255, 255, 255, 0.1);
          }
          .primary-button:hover,
          .ghost-button:hover {
            transform: translateY(-1px);
            filter: brightness(1.04);
          }
          .meta-grid {
            display: grid;
            gap: 14px;
            margin: 0;
          }
          .meta-grid div {
            padding: 14px 0;
            border-bottom: 1px solid rgba(255, 255, 255, 0.08);
          }
          .meta-grid div:last-child {
            border-bottom: 0;
            padding-bottom: 0;
          }
          dt {
            margin: 0 0 4px;
          }
          dd {
            margin: 0;
            line-height: 1.5;
          }
          .info-list {
            display: grid;
            gap: 14px;
          }
          .info-item {
            padding: 16px;
            border-radius: 18px;
            background: rgba(255, 255, 255, 0.05);
            border: 1px solid rgba(255, 255, 255, 0.08);
          }
          @media (max-width: 960px) {
            .hero-panel,
            .layout-grid,
            .login-grid {
              grid-template-columns: 1fr;
            }
            .topbar {
              flex-direction: column;
              align-items: flex-start;
            }
          }
          @media (max-width: 680px) {
            .site-shell {
              padding: 18px 12px 28px;
            }
            .hero-panel,
            .panel {
              padding: 18px;
              border-radius: 20px;
            }
            .provider-grid {
              grid-template-columns: 1fr;
            }
            .hero-stats {
              justify-content: stretch;
            }
            .stat-card {
              width: 100%;
            }
          }
        </style>
      </head>
      <body>
        \(content)
        \(scriptBlock)
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

private struct SessionIdentityRow: Decodable {
    let id: Int64
}

private struct SettingsLoginForm: Content {
    let api_key: String
}

private struct SettingsForm: Content {
    let api_key: String?
    let provider: String
    let qwen_model: String?
    let openai_model: String?
    let openai_api_key: String?
    let clear_openai_key: String?
}
