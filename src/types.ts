export const ATOM_TYPES = ["usercreated", "aicreated", "imported"] as const;
export type AtomType = (typeof ATOM_TYPES)[number];

export interface EmbeddingProvenance {
  provider: string;
  model: string;
  dimensions: number;
  embeddedAt: Date;
}

export type AtomStatus = "active" | "superseded" | "forgotten";

export interface Atom {
  id: string;
  scope: { kind: "global" | "project" | "conversation"; key: string };
  status: AtomStatus;
  validFrom: Date | null;
  expiresAt: Date | null;
  content: string;
  normalizedContent: string;
  tags: string[];
  type: AtomType;
  metadata: Record<string, unknown>;
  embedding: EmbeddingProvenance | null;
  createdAt: Date;
  updatedAt: Date;
}

export interface Vault {
  id: string;
  vaultUid: string;
  name: string | null;
  metadata: Record<string, unknown>;
  createdAt: Date;
  updatedAt: Date;
}

export interface FileObject {
  id: string;
  vaultId: string | null;
  name: string;
  mimeType: string | null;
  base64: string | null;
  content: string | null;
  interpreted: string | null;
  subject: string | null;
  tags: string[];
  subjectEmbedding: EmbeddingProvenance | null;
  contentEmbedding: EmbeddingProvenance | null;
  metadata: Record<string, unknown>;
  createdAt: Date;
  modifiedAt: Date;
}

export interface FileLink {
  id: string;
  aFileId: string;
  bFileId: string;
  subject: string | null;
  absolute: boolean;
  content: string | null;
  createdAt: Date;
  modifiedAt: Date;
}

export interface Modification {
  id: string;
  vaultId: string | null;
  fileId: string | null;
  action: string;
  payload: Record<string, unknown>;
  time: Date;
}

export interface ExtractedAtom {
  content: string;
  tags: string[];
  validFrom?: string | null;
  expires?: string | null;
  subject?: string | null;
  confidence?: number | null;
  memoryType?: "Preference" | "Fact" | "Project" | "Decision" | "Context" | "Instruction" | null;
  scope?: "Global" | "Project" | "Conversation" | null;
}
export interface ExtractionResult { subject: string | null; tags: string[]; atoms: ExtractedAtom[]; }

export interface SearchSignals {
  exact: number;
  lexical: number;
  symbol: number;
  fuzzy: number;
  semantic: number;
  tag: number;
  metadata: number;
}

export interface SearchItem {
  kind: "file" | "atom";
  /** Atom id for kind "atom" (always forgettable), File id for kind "file". Never mixed. */
  id: string;
  score: number;
  rawText: string | null;
  fileId?: string;
  atomId?: string;
  matchedBy: string[];
  scope?: { kind: "global" | "project" | "conversation"; key: string };
  /** Temporal state relative to the query time; only "current" unless history/asOf was requested. */
  state?: "current" | "superseded" | "expired" | "future";
  validFrom?: string | null;
  expires?: string | null;
  /** Source Files this Atom is docked to (provenance). */
  sources?: Array<{ fileId: string; name?: string | null }>;
  /** Active Atoms recorded as contradicting this one; none is auto-resolved. */
  conflictsWith?: string[];
  signals?: SearchSignals;
}
