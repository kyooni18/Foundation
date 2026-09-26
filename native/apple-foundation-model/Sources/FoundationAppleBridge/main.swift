import Foundation
import FoundationModels

@Generable(description: "One independently useful proposition extracted from a raw note.")
struct GeneratedAtom {
    @Guide(description: "Exactly one proposition. Preserve uncertainty and the user's viewpoint. Do not invent facts.")
    var content: String

    @Guide(description: "Short retrieval tags for this proposition.")
    var tags: [String]

    @Guide(description: "ISO-8601 instant for when this proposition became valid, if supported by the note and source time.")
    var validFrom: String?

    @Guide(description: "ISO-8601 expiry instant only for genuinely temporary knowledge; otherwise nil.")
    var expires: String?

    @Guide(description: "Concise subject for this proposition, if useful.")
    var subject: String?

    @Guide(description: "Confidence from 0.0 through 1.0 based only on what the raw note supports.")
    var confidence: Double?

    @Guide(description: "One of Preference, Fact, Project, Decision, Context, Instruction.")
    var memoryType: String?

    @Guide(description: "One of Global, Project, Conversation.")
    var scope: String?
}

@Generable(description: "A short retrieval subject plus a decomposition of the raw personal note into minimal independently useful knowledge.")
struct GeneratedExtraction {
    @Guide(description: "A concise, non-empty internal topic that helps retrieve the raw note later.")
    var subject: String

    @Guide(description: "Short retrieval tags describing the whole note.")
    var tags: [String]

    @Guide(description: "Several minimal propositions when the note contains several useful relations. Do not collapse place, entity, interaction, and evaluation into one paraphrase if they are separately supported.")
    var atoms: [GeneratedAtom]
}

@Generable(description: "A reconciliation decision between one incoming durable memory and existing memory candidates.")
struct GeneratedMemoryResolution {
    @Guide(description: "Exactly one of: new, duplicate, update, supersede, conflict.")
    var action: String

    @Guide(description: "Candidate Atom id selected for duplicate, update, supersede, or conflict. Nil for new.")
    var targetAtomId: String?

    @Guide(description: "Canonical wording for the incoming durable memory. Preserve its meaning and uncertainty.")
    var canonicalContent: String

    @Guide(description: "Short retrieval tags for the canonical memory.")
    var tags: [String]

    @Guide(description: "Confidence in this reconciliation decision from 0.0 through 1.0.")
    var confidence: Double

    @Guide(description: "A concise reason for the reconciliation decision.")
    var rationale: String
}

struct BridgeCandidate: Codable {
    let id: String
    let content: String
    let tags: [String]
    let active: Bool
    let similarity: Double
    let createdAt: String
    let updatedAt: String
    let metadataJSON: String?
}

struct BridgeRequest: Decodable {
    let operation: String
    let text: String
    let sourceTime: String?
    let timeZone: String?
    let tags: [String]?
    let metadataJSON: String?
    let candidates: [BridgeCandidate]?
}

struct BridgeAtom: Encodable {
    let content: String
    let tags: [String]
    let validFrom: String?
    let expires: String?
    let subject: String?
    let confidence: Double?
    let memoryType: String?
    let scope: String?
}

struct BridgeExtraction: Encodable {
    let subject: String?
    let tags: [String]
    let atoms: [BridgeAtom]
}

struct BridgeMemoryResolution: Encodable {
    let action: String
    let targetAtomId: String?
    let canonicalContent: String
    let tags: [String]
    let confidence: Double
    let rationale: String
}

struct BridgeResponse: Encodable {
    let ok: Bool
    let extraction: BridgeExtraction?
    let resolution: BridgeMemoryResolution?
    let error: String?
}

enum BridgeFailure: LocalizedError {
    case unsupportedOperation(String)
    case emptyText
    case modelUnavailable(String)

    var errorDescription: String? {
        switch self {
        case .unsupportedOperation(let operation):
            return "Unsupported operation: \(operation)"
        case .emptyText:
            return "Input text must not be empty"
        case .modelUnavailable(let reason):
            return "Apple Foundation Model is unavailable: \(reason)"
        }
    }
}

func unavailableReason(_ reason: SystemLanguageModel.Availability.UnavailableReason) -> String {
    switch reason {
    case .appleIntelligenceNotEnabled:
        return "Apple Intelligence is not enabled"
    case .deviceNotEligible:
        return "this Mac is not eligible for Apple Intelligence"
    case .modelNotReady:
        return "the on-device model is not ready"
    @unknown default:
        return "unknown availability reason"
    }
}

func availableModel() throws -> SystemLanguageModel {
    let model = SystemLanguageModel.default
    switch model.availability {
    case .available:
        return model
    case .unavailable(let reason):
        throw BridgeFailure.modelUnavailable(unavailableReason(reason))
    }
}

func extractNote(
    _ text: String,
    sourceTime: String?,
    timeZone: String?
) async throws -> BridgeExtraction {
    let trimmed = text.trimmingCharacters(in: .whitespacesAndNewlines)
    guard !trimmed.isEmpty else {
        throw BridgeFailure.emptyText
    }

    let model = try availableModel()
    let resolvedSourceTime = sourceTime ?? ISO8601DateFormatter().string(from: Date())
    let resolvedTimeZone = timeZone ?? TimeZone.current.identifier

    let session = LanguageModelSession(model: model) {
        """
        Extract a concise subject, retrieval tags, and the smallest independently useful facts supported by the person's own raw note.
        Do not simply paraphrase the whole note into one atom when it contains multiple relations.
        Terse personal notes often omit subjects and verbs. When the wording supports firsthand experience, separate useful relations such as where the person was, what named place or entity exists there, what the person did with it, and how the person evaluated it.
        Preserve uncertainty and viewpoint. Never invent unsupported details.

        The source note was captured at \(resolvedSourceTime) in time zone \(resolvedTimeZone).
        Resolve relative temporal phrases against that source time and store a supported ISO-8601 instant in validFrom.
        Only set expires for genuinely temporary knowledge.
        Use memoryType from Preference, Fact, Project, Decision, Context, Instruction and scope from Global, Project, Conversation.
        """
    }

    let response = try await session.respond(
        to: trimmed,
        generating: GeneratedExtraction.self
    )

    let generated = response.content
    let subject = generated.subject.trimmingCharacters(in: .whitespacesAndNewlines)

    return BridgeExtraction(
        subject: subject.isEmpty ? nil : subject,
        tags: generated.tags,
        atoms: generated.atoms.map {
            BridgeAtom(
                content: $0.content,
                tags: $0.tags,
                validFrom: $0.validFrom,
                expires: $0.expires,
                subject: $0.subject,
                confidence: $0.confidence,
                memoryType: $0.memoryType,
                scope: $0.scope
            )
        }
    )
}

func resolveMemory(
    _ text: String,
    sourceTime: String?,
    timeZone: String?,
    tags: [String],
    metadataJSON: String?,
    candidates: [BridgeCandidate]
) async throws -> BridgeMemoryResolution {
    let trimmed = text.trimmingCharacters(in: .whitespacesAndNewlines)
    guard !trimmed.isEmpty else {
        throw BridgeFailure.emptyText
    }

    let model = try availableModel()
    let resolvedSourceTime = sourceTime ?? ISO8601DateFormatter().string(from: Date())
    let resolvedTimeZone = timeZone ?? TimeZone.current.identifier
    let candidateData = try JSONEncoder().encode(candidates)
    let candidateJSON = String(data: candidateData, encoding: .utf8) ?? "[]"

    let session = LanguageModelSession(model: model) {
        """
        Reconcile one incoming durable personal memory with existing candidate memories.

        Choose exactly one action:
        - new: independent knowledge; no candidate represents the same mutable slot or fact.
        - duplicate: materially the same fact; preserve the existing Atom and only refresh metadata/tags.
        - update: the same underlying fact where compatible newer or more precise wording can replace the candidate in place without losing a meaningful historical state.
        - supersede: newer information replaces an older mutable fact, preference, decision, status, path, ownership, setting, or other time-varying value. The caller will preserve the old Atom as inactive history.
        - conflict: statements are materially incompatible but recency or authority is insufficient to decide which is current. The caller will preserve both.

        Use source time, candidate timestamps, and metadata when judging recency.
        Do not supersede timeless facts merely because wording differs.
        Do not treat topical similarity as identity.
        Prefer duplicate over update when no material information changes.
        Prefer supersede over update when the old state is historically meaningful.
        If uncertain, choose conflict or new rather than silently overwriting.
        targetAtomId must be one supplied candidate id for every action except new.
        """
    }

    let prompt = """
    Incoming memory: \(trimmed)
    Incoming tags: \(tags.joined(separator: ", "))
    Incoming metadata JSON: \(metadataJSON ?? "{}")
    Source time: \(resolvedSourceTime)
    Time zone: \(resolvedTimeZone)

    Candidate memories JSON:
    \(candidateJSON)
    """

    let response = try await session.respond(
        to: prompt,
        generating: GeneratedMemoryResolution.self
    )
    let generated = response.content
    let canonical = generated.canonicalContent.trimmingCharacters(in: .whitespacesAndNewlines)
    let rationale = generated.rationale.trimmingCharacters(in: .whitespacesAndNewlines)

    return BridgeMemoryResolution(
        action: generated.action.trimmingCharacters(in: .whitespacesAndNewlines).lowercased(),
        targetAtomId: generated.targetAtomId,
        canonicalContent: canonical.isEmpty ? trimmed : canonical,
        tags: generated.tags,
        confidence: min(1.0, max(0.0, generated.confidence)),
        rationale: rationale
    )
}

func writeResponse(_ response: BridgeResponse) {
    do {
        let data = try JSONEncoder().encode(response)
        FileHandle.standardOutput.write(data)
        FileHandle.standardOutput.write(Data([0x0A]))
    } catch {
        FileHandle.standardError.write(Data("Failed to encode bridge response: \(error)\n".utf8))
    }
}

@main
struct FoundationAppleBridge {
    static func main() async {
        do {
            let data = FileHandle.standardInput.readDataToEndOfFile()
            let request = try JSONDecoder().decode(BridgeRequest.self, from: data)

            var extraction: BridgeExtraction?
            var resolution: BridgeMemoryResolution?

            switch request.operation {
            case "extractNote":
                extraction = try await extractNote(
                    request.text,
                    sourceTime: request.sourceTime,
                    timeZone: request.timeZone
                )
            case "resolveMemory":
                resolution = try await resolveMemory(
                    request.text,
                    sourceTime: request.sourceTime,
                    timeZone: request.timeZone,
                    tags: request.tags ?? [],
                    metadataJSON: request.metadataJSON,
                    candidates: request.candidates ?? []
                )
            default:
                throw BridgeFailure.unsupportedOperation(request.operation)
            }

            writeResponse(
                BridgeResponse(
                    ok: true,
                    extraction: extraction,
                    resolution: resolution,
                    error: nil
                )
            )
        } catch {
            writeResponse(
                BridgeResponse(
                    ok: false,
                    extraction: nil,
                    resolution: nil,
                    error: error.localizedDescription
                )
            )
            Foundation.exit(EXIT_FAILURE)
        }
    }
}
