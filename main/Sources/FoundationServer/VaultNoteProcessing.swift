import Crypto
import FluentSQL
import Foundation
import Vapor

private let managedRelatedLinksStart = "<!-- foundation:related-links:start -->"
private let managedRelatedLinksEnd = "<!-- foundation:related-links:end -->"
private let noteSimilarityDistanceThreshold = 0.32
private let noteSimilarityNeighborLimit = 8
private let noteSimilarityScanLimit = 24
private let noteKeypointLimit = 128
private let noteProcessingDeviceID = "foundation-cip"

struct VaultProcessingSummary {
    let latestChangeID: Int64?
    let latestChangeUnixMS: Int64?
}

func processVaultNotes(
    vaultID: Int64,
    vaultUID: String,
    seedFileIDs: Set<Int64>,
    sql: any SQLDatabase,
    context: AppContext,
    settings: EmbeddingSettings
) async throws -> VaultProcessingSummary {
    guard !seedFileIDs.isEmpty else {
        return VaultProcessingSummary(latestChangeID: nil, latestChangeUnixMS: nil)
    }

    do {
        let seedRows = try await loadVaultNoteRows(vaultID: vaultID, fileIDs: seedFileIDs, sql: sql)
        let oldNeighborIDs = try await loadExistingNeighborIDs(seedFileIDs: seedFileIDs, sql: sql)

        var preparedNotes: [PreparedNoteUpdate] = []
        preparedNotes.reserveCapacity(seedRows.count)

        for row in seedRows where isProcessableMarkdownNote(row) {
            let baseContent = removingManagedRelatedLinksBlock(from: row.content ?? "")
            let subject = resolvedNoteTitle(filePath: row.file_path, content: baseContent)
            let keypoints = extractAtomicKeypoints(from: baseContent, fallbackTitle: subject)
            let interpreted = keypoints.joined(separator: "\n")
            let contentSummary = normalizedNonEmpty(interpreted) ?? subject
            let embeddingInputs = [subject, contentSummary] + keypoints
            let embeddings = try await context.embeddingService.embedMany(embeddingInputs, settings: settings)

            preparedNotes.append(
                PreparedNoteUpdate(
                    fileID: row.id,
                    filePath: row.file_path,
                    subject: subject,
                    interpreted: interpreted,
                    subjectVector: embeddings[0],
                    contentVector: embeddings[1],
                    keypoints: zip(keypoints, embeddings.dropFirst(2)).map { KeypointEmbedding(text: $0.0, embedding: $0.1) },
                    parentKey: "vault://\(vaultUID)/\(row.file_path)"
                )
            )
        }

        let summary = try await applyPreparedNoteUpdates(
            vaultID: vaultID,
            vaultUID: vaultUID,
            seedRows: seedRows,
            preparedNotes: preparedNotes,
            oldNeighborIDs: oldNeighborIDs,
            sql: sql,
            context: context
        )

        try await markVaultFileJobsCompleted(fileIDs: seedFileIDs, sql: sql)
        return summary
    } catch {
        try? await markVaultFileJobsFailed(fileIDs: seedFileIDs, sql: sql, message: error.localizedDescription)
        throw error
    }
}

func semanticSearchVaultNotes(
    vaultUID: String,
    query: String,
    limit: Int,
    sql: any SQLDatabase,
    context: AppContext,
    settings: EmbeddingSettings
) async throws -> [VaultSemanticSearchResultItem] {
    let atomsTable = context.config.embeddingsTable
    let embedding = try await context.embeddingService.embed(query, settings: settings)
    let vectorLiteral = context.embeddingService.vectorLiteral(for: embedding)

    let rows = try await sql
        .raw(
            """
            SELECT
                ranked.file_path,
                ranked.title,
                ranked.keypoint,
                ranked.distance,
                ranked.updated_unix_ms
            FROM (
                SELECT
                    vf.id AS file_id,
                    vf.file_path,
                    vf.subject AS title,
                    a.content AS keypoint,
                    vf.updated_unix_ms,
                    (a.vector <-> (\(bind: vectorLiteral))::vector)::double precision AS distance,
                    ROW_NUMBER() OVER (
                        PARTITION BY vf.id
                        ORDER BY a.vector <-> (\(bind: vectorLiteral))::vector ASC, a.id ASC
                    ) AS row_rank
                FROM vaults v
                JOIN vault_files vf ON vf.vault_id = v.id
                JOIN file_atoms fa ON fa.file_id = vf.id
                JOIN \(ident: atomsTable) a ON a.id = fa.atom_id
                WHERE v.vault_uid = \(bind: vaultUID)
                  AND vf.is_deleted = FALSE
                  AND LOWER(vf.file_path) LIKE '%.md'
            ) ranked
            WHERE ranked.row_rank = 1
            ORDER BY ranked.distance ASC, ranked.file_path ASC
            LIMIT \(bind: limit)
            """
        )
        .all(decoding: VaultSemanticSearchRow.self)

    return rows.map {
        VaultSemanticSearchResultItem(
            file_path: $0.file_path,
            title: normalizedNonEmpty($0.title) ?? fileStem(from: $0.file_path),
            keypoint: $0.keypoint,
            distance: $0.distance,
            updated_unix_ms: $0.updated_unix_ms,
            obsidian_link: obsidianLinkTarget(for: $0.file_path)
        )
    }
}

private func applyPreparedNoteUpdates(
    vaultID: Int64,
    vaultUID: String,
    seedRows: [VaultNoteRow],
    preparedNotes: [PreparedNoteUpdate],
    oldNeighborIDs: Set<Int64>,
    sql: any SQLDatabase,
    context: AppContext
) async throws -> VaultProcessingSummary {
    let atomsTable = context.config.embeddingsTable

    for prepared in preparedNotes {
        try await updatePreparedNoteFields(prepared, sql: sql, context: context)
        try await insertPreparedNoteAtoms(prepared, vaultUID: vaultUID, sql: sql, atomsTable: atomsTable, context: context)
    }

    let seedFileIDs = Set(seedRows.map(\.id))
    try await deleteExistingSimilarityLinks(seedFileIDs: seedFileIDs, sql: sql)

    var newNeighborIDs = Set<Int64>()
    var insertedPairs = Set<FileLinkPair>()
    for prepared in preparedNotes {
        let neighbors = try await findSimilarNeighbors(
            vaultID: vaultID,
            fileID: prepared.fileID,
            contentVector: prepared.contentVector,
            sql: sql,
            context: context
        )

        for neighbor in neighbors {
            newNeighborIDs.insert(neighbor.id)
            let pair = FileLinkPair(prepared.fileID, neighbor.id)
            if insertedPairs.contains(pair) {
                continue
            }

            insertedPairs.insert(pair)
            try await upsertSimilarityLink(
                pair: pair,
                similarity: max(0, 1 - neighbor.distance),
                sql: sql
            )
        }
    }

    let impactedFileIDs = seedFileIDs.union(oldNeighborIDs).union(newNeighborIDs)
    guard !impactedFileIDs.isEmpty else {
        return VaultProcessingSummary(latestChangeID: nil, latestChangeUnixMS: nil)
    }

    let impactedRows = try await loadVaultNoteRows(vaultID: vaultID, fileIDs: impactedFileIDs, sql: sql)
    let nowUnixMS = unixMillisecondsNow()
    var latestChange = VaultProcessingSummary(latestChangeID: nil, latestChangeUnixMS: nil)

    for row in impactedRows {
        guard isProcessableMarkdownNote(row), let currentContent = row.content else {
            continue
        }

        let relatedNotes = try await loadRelatedNotes(for: row.id, sql: sql)
        let renderedContent = applyingManagedRelatedLinks(relatedNotes.map(\.file_path), to: currentContent)
        guard renderedContent != currentContent else {
            continue
        }

        let renderedData = Data(renderedContent.utf8)
        let checksum = SHA256.hash(data: renderedData).map { String(format: "%02x", $0) }.joined()
        let base64 = renderedData.base64EncodedString()
        let changeRows = try await sql
            .raw(
                """
                INSERT INTO vault_changes (
                    vault_id,
                    file_id,
                    device_id,
                    file_path,
                    action,
                    changed_at_unix_ms,
                    content_base64,
                    content_sha256,
                    size_bytes
                ) VALUES (
                    \(bind: vaultID),
                    \(bind: row.id),
                    \(bind: noteProcessingDeviceID),
                    \(bind: row.file_path),
                    \(bind: VaultChangeAction.modified.rawValue),
                    \(bind: nowUnixMS),
                    \(bind: base64),
                    \(bind: checksum),
                    \(bind: Int64(renderedData.count))
                )
                RETURNING id, changed_at_unix_ms
                """
            )
            .all(decoding: VaultProcessingChangeRow.self)

        guard let changeRow = changeRows.first else {
            throw Abort(.internalServerError, reason: "failed to insert generated vault change")
        }

        try await sql
            .raw(
                """
                UPDATE vault_files
                SET
                    base64 = \(bind: base64),
                    content = \(bind: renderedContent),
                    content_sha256 = \(bind: checksum),
                    size_bytes = \(bind: Int64(renderedData.count)),
                    updated_unix_ms = \(bind: changeRow.changed_at_unix_ms),
                    modified_at = NOW(),
                    updated_at = NOW(),
                    last_change_id = \(bind: changeRow.id)
                WHERE id = \(bind: row.id)
                """
            )
            .run()

        try persistProcessedVaultFile(vaultUID: vaultUID, filePath: row.file_path, content: renderedData)

        if isChangeLater(candidate: changeRow, than: latestChange) {
            latestChange = VaultProcessingSummary(
                latestChangeID: changeRow.id,
                latestChangeUnixMS: changeRow.changed_at_unix_ms
            )
        }
    }

    return latestChange
}

private func updatePreparedNoteFields(
    _ prepared: PreparedNoteUpdate,
    sql: any SQLDatabase,
    context: AppContext
) async throws {
    let subjectLiteral = context.embeddingService.vectorLiteral(for: prepared.subjectVector)
    let contentLiteral = context.embeddingService.vectorLiteral(for: prepared.contentVector)

    try await sql
        .raw(
            """
            UPDATE vault_files
            SET
                subject = NULLIF(\(bind: prepared.subject), ''),
                interpreted = NULLIF(\(bind: prepared.interpreted), ''),
                vector_subject = (\(bind: subjectLiteral))::vector,
                vector_content = (\(bind: contentLiteral))::vector,
                updated_at = NOW()
            WHERE id = \(bind: prepared.fileID)
            """
        )
        .run()
}

private func insertPreparedNoteAtoms(
    _ prepared: PreparedNoteUpdate,
    vaultUID: String,
    sql: any SQLDatabase,
    atomsTable: String,
    context: AppContext
) async throws {
    for (index, keypoint) in prepared.keypoints.enumerated() {
        let vectorLiteral = context.embeddingService.vectorLiteral(for: keypoint.embedding)
        let metadata = try serializedAtomMetadata(
            vaultUID: vaultUID,
            filePath: prepared.filePath,
            title: prepared.subject,
            keypointIndex: index
        )

        let rows = try await sql
            .raw(
                """
                INSERT INTO \(ident: atomsTable) (content, vector, type, parent, metadata)
                VALUES (
                    \(bind: keypoint.text),
                    (\(bind: vectorLiteral))::vector,
                    \(bind: "imported"),
                    \(bind: prepared.parentKey),
                    (\(bind: metadata))::jsonb
                )
                RETURNING id
                """
            )
            .all(decoding: VaultAtomRow.self)

        guard let atomRow = rows.first else {
            throw Abort(.internalServerError, reason: "failed to insert keypoint atom")
        }

        try await sql
            .raw(
                """
                INSERT INTO file_atoms (file_id, atom_id)
                VALUES (\(bind: prepared.fileID), \(bind: atomRow.id))
                ON CONFLICT (file_id, atom_id) DO NOTHING
                """
            )
            .run()
    }
}

private func deleteExistingSimilarityLinks(seedFileIDs: Set<Int64>, sql: any SQLDatabase) async throws {
    guard let idList = unsafeIDList(seedFileIDs) else {
        return
    }

    try await sql
        .raw(
            """
            DELETE FROM file_links
            WHERE file_a_id IN (\(unsafeRaw: idList))
               OR file_b_id IN (\(unsafeRaw: idList))
            """
        )
        .run()
}

private func findSimilarNeighbors(
    vaultID: Int64,
    fileID: Int64,
    contentVector: [Double],
    sql: any SQLDatabase,
    context: AppContext
) async throws -> [NeighborDistanceRow] {
    let vectorLiteral = context.embeddingService.vectorLiteral(for: contentVector)
    let rows = try await sql
        .raw(
            """
            SELECT
                vf.id,
                vf.file_path,
                (vf.vector_content <-> (\(bind: vectorLiteral))::vector)::double precision AS distance
            FROM vault_files vf
            WHERE vf.vault_id = \(bind: vaultID)
              AND vf.id <> \(bind: fileID)
              AND vf.is_deleted = FALSE
              AND vf.vector_content IS NOT NULL
              AND LOWER(vf.file_path) LIKE '%.md'
            ORDER BY vf.vector_content <-> (\(bind: vectorLiteral))::vector
            LIMIT \(bind: noteSimilarityScanLimit)
            """
        )
        .all(decoding: NeighborDistanceRow.self)

    return rows
        .filter { $0.distance <= noteSimilarityDistanceThreshold }
        .prefix(noteSimilarityNeighborLimit)
        .map { $0 }
}

private func upsertSimilarityLink(pair: FileLinkPair, similarity: Double, sql: any SQLDatabase) async throws {
    try await sql
        .raw(
            """
            INSERT INTO file_links (file_a_id, file_b_id, subject, absolute, atoms, content, updated_at)
            VALUES (
                \(bind: pair.a),
                \(bind: pair.b),
                \(bind: "semantic_similarity"),
                \(bind: similarity),
                ARRAY[]::BIGINT[],
                NULL,
                NOW()
            )
            ON CONFLICT (file_a_id, file_b_id)
            DO UPDATE SET
                subject = EXCLUDED.subject,
                absolute = EXCLUDED.absolute,
                atoms = EXCLUDED.atoms,
                content = EXCLUDED.content,
                updated_at = NOW()
            """
        )
        .run()
}

private func loadVaultNoteRows(vaultID: Int64, fileIDs: Set<Int64>, sql: any SQLDatabase) async throws -> [VaultNoteRow] {
    guard let idList = unsafeIDList(fileIDs) else {
        return []
    }

    return try await sql
        .raw(
            """
            SELECT
                id,
                file_path,
                content,
                is_deleted,
                content_version
            FROM vault_files
            WHERE vault_id = \(bind: vaultID)
              AND id IN (\(unsafeRaw: idList))
            ORDER BY id ASC
            """
        )
        .all(decoding: VaultNoteRow.self)
}

private func loadExistingNeighborIDs(seedFileIDs: Set<Int64>, sql: any SQLDatabase) async throws -> Set<Int64> {
    guard let idList = unsafeIDList(seedFileIDs) else {
        return []
    }

    let rows = try await sql
        .raw(
            """
            SELECT
                CASE
                    WHEN file_a_id IN (\(unsafeRaw: idList)) THEN file_b_id
                    ELSE file_a_id
                END AS id
            FROM file_links
            WHERE file_a_id IN (\(unsafeRaw: idList))
               OR file_b_id IN (\(unsafeRaw: idList))
            """
        )
        .all(decoding: VaultIDOnlyRow.self)

    return Set(rows.map(\.id))
}

private func loadRelatedNotes(for fileID: Int64, sql: any SQLDatabase) async throws -> [RelatedNoteRow] {
    try await sql
        .raw(
            """
            SELECT
                other.file_path,
                fl.absolute AS similarity
            FROM file_links fl
            JOIN vault_files other
              ON other.id = CASE
                  WHEN fl.file_a_id = \(bind: fileID) THEN fl.file_b_id
                  ELSE fl.file_a_id
              END
            WHERE (fl.file_a_id = \(bind: fileID) OR fl.file_b_id = \(bind: fileID))
              AND other.is_deleted = FALSE
            ORDER BY fl.absolute DESC, other.file_path ASC
            LIMIT \(bind: noteSimilarityNeighborLimit)
            """
        )
        .all(decoding: RelatedNoteRow.self)
}

private func markVaultFileJobsCompleted(fileIDs: Set<Int64>, sql: any SQLDatabase) async throws {
    guard let idList = unsafeIDList(fileIDs) else {
        return
    }

    try await sql
        .raw(
            """
            UPDATE file_processing_jobs
            SET
                status = 'completed',
                started_at = COALESCE(started_at, NOW()),
                finished_at = NOW(),
                last_error = NULL,
                updated_at = NOW()
            WHERE file_id IN (\(unsafeRaw: idList))
              AND status IN ('pending', 'running', 'failed')
            """
        )
        .run()
}

private func markVaultFileJobsFailed(fileIDs: Set<Int64>, sql: any SQLDatabase, message: String) async throws {
    guard let idList = unsafeIDList(fileIDs) else {
        return
    }

    try await sql
        .raw(
            """
            UPDATE file_processing_jobs
            SET
                status = 'failed',
                attempts = attempts + 1,
                started_at = COALESCE(started_at, NOW()),
                finished_at = NOW(),
                last_error = \(bind: message),
                updated_at = NOW()
            WHERE file_id IN (\(unsafeRaw: idList))
              AND status IN ('pending', 'running')
            """
        )
        .run()
}

private func persistProcessedVaultFile(vaultUID: String, filePath: String, content: Data) throws {
    let root = processingVaultRoot(vaultUID: vaultUID)
    let fileURL = processingVaultFileURL(vaultRoot: root, filePath: filePath)
    try FileManager.default.createDirectory(at: fileURL.deletingLastPathComponent(), withIntermediateDirectories: true)
    try content.write(to: fileURL, options: .atomic)
}

private func processingVaultRoot(vaultUID: String) -> URL {
    let cwd = URL(fileURLWithPath: FileManager.default.currentDirectoryPath, isDirectory: true)
    return cwd
        .appendingPathComponent("vault_storage", isDirectory: true)
        .appendingPathComponent(vaultUID, isDirectory: true)
}

private func processingVaultFileURL(vaultRoot: URL, filePath: String) -> URL {
    filePath
        .split(separator: "/", omittingEmptySubsequences: true)
        .reduce(vaultRoot) { partial, segment in
            partial.appendingPathComponent(String(segment), isDirectory: false)
        }
}

private func serializedAtomMetadata(
    vaultUID: String,
    filePath: String,
    title: String,
    keypointIndex: Int
) throws -> String {
    let metadata: [String: Any] = [
        "vault_uid": vaultUID,
        "file_path": filePath,
        "title": title,
        "keypoint_index": keypointIndex
    ]
    let data = try JSONSerialization.data(withJSONObject: metadata, options: [.sortedKeys])
    guard let json = String(data: data, encoding: .utf8) else {
        throw Abort(.internalServerError, reason: "failed to encode keypoint metadata")
    }
    return json
}

private func isProcessableMarkdownNote(_ row: VaultNoteRow) -> Bool {
    !row.is_deleted && row.content != nil && row.file_path.lowercased().hasSuffix(".md")
}

private func removingManagedRelatedLinksBlock(from content: String) -> String {
    guard let startRange = content.range(of: managedRelatedLinksStart),
          let endRange = content.range(of: managedRelatedLinksEnd, range: startRange.upperBound..<content.endIndex)
    else {
        return content
    }

    let hadTrailingNewline = content.hasSuffix("\n")
    let prefix = content[..<startRange.lowerBound]
    let suffix = content[endRange.upperBound...]
    var combined = String(prefix) + String(suffix)
    combined = combined.replacingOccurrences(of: "\r\n", with: "\n")
    while combined.contains("\n\n\n") {
        combined = combined.replacingOccurrences(of: "\n\n\n", with: "\n\n")
    }
    combined = combined.trimmingCharacters(in: .whitespacesAndNewlines)
    if hadTrailingNewline, !combined.isEmpty {
        combined.append("\n")
    }
    return combined
}

private func applyingManagedRelatedLinks(_ relatedFilePaths: [String], to content: String) -> String {
    let baseContent = removingManagedRelatedLinksBlock(from: content).trimmingCharacters(in: .whitespacesAndNewlines)
    let links = Array(
        LinkedHashSet(
            relatedFilePaths.map(obsidianLinkTarget(for:))
        )
    )

    guard !links.isEmpty else {
        return baseContent.isEmpty ? "" : baseContent + "\n"
    }

    let blockLines = [
        managedRelatedLinksStart,
        "## Related Notes"
    ] + links.map { "- [[\($0)]]" } + [managedRelatedLinksEnd]

    let block = blockLines.joined(separator: "\n")
    guard !baseContent.isEmpty else {
        return block + "\n"
    }

    return baseContent + "\n\n" + block + "\n"
}

private func obsidianLinkTarget(for filePath: String) -> String {
    let normalized = filePath.precomposedStringWithCanonicalMapping
    let stem = (normalized as NSString).deletingPathExtension
    return stem.replacingOccurrences(of: "\\", with: "/")
}

private func resolvedNoteTitle(filePath: String, content: String) -> String {
    let frontmatter = splitFrontmatter(from: content)
    if let title = frontmatter.metadata["title"].flatMap(normalizedNonEmpty) {
        return title
    }

    for rawLine in frontmatter.body.components(separatedBy: .newlines) {
        let line = rawLine.trimmingCharacters(in: .whitespaces)
        if let heading = parseHeading(line) {
            return heading.text
        }
    }

    return fileStem(from: filePath).precomposedStringWithCanonicalMapping
}

private func extractAtomicKeypoints(from content: String, fallbackTitle: String) -> [String] {
    let parsed = splitFrontmatter(from: content)
    let body = parsed.body.replacingOccurrences(of: "\r\n", with: "\n")
    var inCodeBlock = false
    var headingStack: [(level: Int, text: String)] = []
    var paragraphBuffer: [String] = []
    var results: [String] = []
    var seen = Set<String>()

    func appendKeypoint(_ raw: String) {
        guard var text = normalizedNonEmpty(normalizeMarkdownInline(raw)) else {
            return
        }

        if text.count > 420 {
            text = String(text.prefix(420)).trimmingCharacters(in: .whitespacesAndNewlines)
        }

        let contextPrefix = headingStack.map(\.text).joined(separator: " / ")
        let candidate = contextPrefix.isEmpty || text.hasPrefix(contextPrefix) ? text : "\(contextPrefix): \(text)"
        let normalized = candidate.precomposedStringWithCanonicalMapping
        guard !seen.contains(normalized) else {
            return
        }
        seen.insert(normalized)
        results.append(normalized)
    }

    func flushParagraph() {
        guard !paragraphBuffer.isEmpty else {
            return
        }

        let paragraph = paragraphBuffer.joined(separator: " ")
        paragraphBuffer.removeAll(keepingCapacity: true)
        for sentence in splitIntoSentences(normalizeMarkdownInline(paragraph)) {
            appendKeypoint(sentence)
            if results.count >= noteKeypointLimit {
                return
            }
        }
    }

    for rawLine in body.components(separatedBy: .newlines) {
        let trimmed = rawLine.trimmingCharacters(in: .whitespaces)

        if isFenceLine(trimmed) {
            flushParagraph()
            inCodeBlock.toggle()
            continue
        }
        if inCodeBlock {
            continue
        }

        if trimmed.isEmpty {
            flushParagraph()
            continue
        }

        if let heading = parseHeading(trimmed) {
            flushParagraph()
            while let last = headingStack.last, last.level >= heading.level {
                headingStack.removeLast()
            }
            headingStack.append((heading.level, heading.text))
            continue
        }

        if isMarkdownTableSeparator(trimmed) {
            flushParagraph()
            continue
        }

        if let listItem = parseListItem(trimmed) {
            flushParagraph()
            appendKeypoint(listItem)
        } else if let quote = parseBlockQuote(trimmed) {
            flushParagraph()
            appendKeypoint(quote)
        } else if let tableRow = parseMarkdownTableRow(trimmed) {
            flushParagraph()
            appendKeypoint(tableRow)
        } else {
            paragraphBuffer.append(trimmed)
        }

        if results.count >= noteKeypointLimit {
            break
        }
    }

    flushParagraph()

    if results.isEmpty, let fallback = normalizedNonEmpty(normalizeMarkdownInline(body)) {
        for sentence in splitIntoSentences(fallback) {
            appendKeypoint(sentence)
            if results.count >= noteKeypointLimit {
                break
            }
        }
    }

    if results.isEmpty {
        appendKeypoint(fallbackTitle)
    }

    return Array(results.prefix(noteKeypointLimit))
}

private func splitFrontmatter(from content: String) -> (metadata: [String: String], body: String) {
    let normalized = content.replacingOccurrences(of: "\r\n", with: "\n")
    guard normalized.hasPrefix("---\n") else {
        return ([:], normalized)
    }

    let lines = normalized.components(separatedBy: "\n")
    guard let closingIndex = lines.dropFirst().firstIndex(of: "---") else {
        return ([:], normalized)
    }

    var metadata: [String: String] = [:]
    for line in lines[1..<closingIndex] {
        guard let separator = line.firstIndex(of: ":") else {
            continue
        }
        let key = String(line[..<separator]).trimmingCharacters(in: .whitespacesAndNewlines).lowercased()
        let value = String(line[line.index(after: separator)...]).trimmingCharacters(in: .whitespacesAndNewlines)
        if !key.isEmpty, !value.isEmpty {
            metadata[key] = value
        }
    }

    let body = lines[(closingIndex + 1)...].joined(separator: "\n")
    return (metadata, body)
}

private func parseHeading(_ line: String) -> (level: Int, text: String)? {
    var level = 0
    for character in line {
        if character == "#" {
            level += 1
        } else {
            break
        }
    }

    guard level > 0, level <= 6 else {
        return nil
    }

    let index = line.index(line.startIndex, offsetBy: level)
    guard index < line.endIndex, line[index] == " " else {
        return nil
    }

    let text = line[index...].trimmingCharacters(in: .whitespacesAndNewlines)
    guard let normalized = normalizedNonEmpty(normalizeMarkdownInline(text)) else {
        return nil
    }

    return (level, normalized)
}

private func parseListItem(_ line: String) -> String? {
    let patterns = [
        #"^[-*+]\s+(.*)$"#,
        #"^\d+[.)]\s+(.*)$"#,
        #"^[-*+]\s+\[[ xX]\]\s+(.*)$"#
    ]

    for pattern in patterns {
        if let match = firstCapture(in: line, pattern: pattern) {
            return normalizedNonEmpty(match)
        }
    }

    return nil
}

private func parseBlockQuote(_ line: String) -> String? {
    guard let match = firstCapture(in: line, pattern: #"^>\s*(.*)$"#) else {
        return nil
    }
    return normalizedNonEmpty(match)
}

private func parseMarkdownTableRow(_ line: String) -> String? {
    guard line.contains("|"), !line.hasPrefix("|-") else {
        return nil
    }

    let cells = line
        .split(separator: "|", omittingEmptySubsequences: false)
        .map { $0.trimmingCharacters(in: .whitespacesAndNewlines) }
        .compactMap(normalizedNonEmpty)

    guard cells.count >= 2 else {
        return nil
    }
    return cells.joined(separator: " | ")
}

private func isMarkdownTableSeparator(_ line: String) -> Bool {
    guard line.contains("|") else {
        return false
    }
    return line.replacingOccurrences(of: ":", with: "").allSatisfy { $0 == "|" || $0 == "-" || $0 == " " }
}

private func isFenceLine(_ line: String) -> Bool {
    line.hasPrefix("```") || line.hasPrefix("~~~")
}

private func normalizeMarkdownInline(_ text: String) -> String {
    var value = normalizeObsidianWikiLinks(in: text)
    let replacements: [(String, String)] = [
        (#"\[(.*?)\]\((.*?)\)"#, "$1"),
        (#"`([^`]*)`"#, "$1"),
        (#"[*_~#>]+"#, " "),
        (#"\s+"#, " ")
    ]

    for (pattern, template) in replacements {
        value = replacingRegexMatches(in: value, pattern: pattern, template: template)
    }

    return value.trimmingCharacters(in: .whitespacesAndNewlines)
}

private func normalizeObsidianWikiLinks(in text: String) -> String {
    guard let regex = try? NSRegularExpression(pattern: #"!?\[\[(.*?)(?:\|(.*?))?\]\]"#, options: []) else {
        return text
    }

    let matches = regex.matches(in: text, options: [], range: NSRange(text.startIndex..<text.endIndex, in: text))
    guard !matches.isEmpty else {
        return text
    }

    var normalized = text
    for match in matches.reversed() {
        guard let fullRange = Range(match.range(at: 0), in: normalized) else {
            continue
        }

        let target = match.numberOfRanges > 1 ? Range(match.range(at: 1), in: normalized).map { String(normalized[$0]) } : nil
        let alias = match.numberOfRanges > 2 ? Range(match.range(at: 2), in: normalized).map { String(normalized[$0]) } : nil
        let replacement = normalizedNonEmpty(alias) ?? normalizedNonEmpty(target) ?? ""
        normalized.replaceSubrange(fullRange, with: replacement)
    }

    return normalized
}

private func splitIntoSentences(_ text: String) -> [String] {
    let normalized = text.replacingOccurrences(of: "\n", with: " ").trimmingCharacters(in: .whitespacesAndNewlines)
    guard !normalized.isEmpty else {
        return []
    }

    let enders = CharacterSet(charactersIn: ".!?;。！？")
    var sentences: [String] = []
    var current = ""

    for scalar in normalized.unicodeScalars {
        current.unicodeScalars.append(scalar)
        if enders.contains(scalar) {
            if let sentence = normalizedNonEmpty(current) {
                sentences.append(sentence)
            }
            current.removeAll(keepingCapacity: true)
        }
    }

    if let tail = normalizedNonEmpty(current) {
        sentences.append(tail)
    }

    return sentences.isEmpty ? [normalized] : sentences
}

private func replacingRegexMatches(in text: String, pattern: String, template: String) -> String {
    guard let regex = try? NSRegularExpression(pattern: pattern, options: []) else {
        return text
    }
    let range = NSRange(text.startIndex..<text.endIndex, in: text)
    return regex.stringByReplacingMatches(in: text, options: [], range: range, withTemplate: template)
}

private func firstCapture(in text: String, pattern: String) -> String? {
    guard let regex = try? NSRegularExpression(pattern: pattern, options: []) else {
        return nil
    }
    let range = NSRange(text.startIndex..<text.endIndex, in: text)
    guard let match = regex.firstMatch(in: text, options: [], range: range),
          match.numberOfRanges > 1,
          let captureRange = Range(match.range(at: 1), in: text)
    else {
        return nil
    }
    return String(text[captureRange])
}

private func fileStem(from filePath: String) -> String {
    let name = (filePath as NSString).lastPathComponent
    return (name as NSString).deletingPathExtension
}

private func unsafeIDList(_ ids: Set<Int64>) -> String? {
    guard !ids.isEmpty else {
        return nil
    }
    return ids.sorted().map(String.init).joined(separator: ", ")
}

private func normalizedNonEmpty(_ raw: String?) -> String? {
    guard let raw else {
        return nil
    }
    let trimmed = raw.trimmingCharacters(in: .whitespacesAndNewlines)
    return trimmed.isEmpty ? nil : trimmed
}

private func isChangeLater(candidate: VaultProcessingChangeRow, than summary: VaultProcessingSummary) -> Bool {
    guard let existingUnixMS = summary.latestChangeUnixMS else {
        return true
    }
    if candidate.changed_at_unix_ms != existingUnixMS {
        return candidate.changed_at_unix_ms > existingUnixMS
    }
    return candidate.id > (summary.latestChangeID ?? 0)
}

private struct PreparedNoteUpdate {
    let fileID: Int64
    let filePath: String
    let subject: String
    let interpreted: String
    let subjectVector: [Double]
    let contentVector: [Double]
    let keypoints: [KeypointEmbedding]
    let parentKey: String
}

private struct KeypointEmbedding {
    let text: String
    let embedding: [Double]

    init(text: String, embedding: [Double]) {
        self.text = text
        self.embedding = embedding
    }
}

private struct FileLinkPair: Hashable {
    let a: Int64
    let b: Int64

    init(_ lhs: Int64, _ rhs: Int64) {
        if lhs <= rhs {
            self.a = lhs
            self.b = rhs
        } else {
            self.a = rhs
            self.b = lhs
        }
    }
}

private struct LinkedHashSet<Element: Hashable>: Sequence {
    private var ordered: [Element] = []
    private var seen = Set<Element>()

    init(_ elements: [Element]) {
        for element in elements where !seen.contains(element) {
            seen.insert(element)
            ordered.append(element)
        }
    }

    func makeIterator() -> IndexingIterator<[Element]> {
        ordered.makeIterator()
    }
}

private struct VaultNoteRow: Decodable {
    let id: Int64
    let file_path: String
    let content: String?
    let is_deleted: Bool
    let content_version: Int64
}

private struct VaultAtomRow: Decodable {
    let id: Int64
}

private struct VaultIDOnlyRow: Decodable {
    let id: Int64
}

private struct NeighborDistanceRow: Decodable {
    let id: Int64
    let file_path: String
    let distance: Double
}

private struct RelatedNoteRow: Decodable {
    let file_path: String
    let similarity: Double
}

private struct VaultProcessingChangeRow: Decodable {
    let id: Int64
    let changed_at_unix_ms: Int64
}

private struct VaultSemanticSearchRow: Decodable {
    let file_path: String
    let title: String?
    let keypoint: String
    let distance: Double
    let updated_unix_ms: Int64
}
