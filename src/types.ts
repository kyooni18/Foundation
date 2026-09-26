export const ATOM_TYPES = ["usercreated", "aicreated", "imported"] as const;
export type AtomType = (typeof ATOM_TYPES)[number];

export interface EmbeddingProvenance {
  provider: string;
  model: string;
  dimensions: number;
  embeddedAt: Date;
}

export interface Atom {
  id: string;
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

export interface SearchItem {
  kind: "file" | "atom";
  id: string;
  score: number;
  rawText: string | null;
  fileId?: string;
  atomId?: string;
  matchedBy: string[];
}
