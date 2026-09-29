import { describe, expect, it } from "vitest";
import { SCHEMA_VERSION, schemaStatements } from "../src/schema.js";

describe("Foundation schema", () => {
  it("keeps Atom top-level and File docking relational", () => {
    const sql = schemaStatements.join("\n").toLowerCase();
    const atomTable = schemaStatements.find(statement => statement.toLowerCase().includes("create table if not exists atoms"))!.toLowerCase();
    expect(SCHEMA_VERSION).toBe(6);
    expect(sql).toContain("create table if not exists atoms");
    expect(sql).toContain("create table if not exists atom_file_docks");
    expect(atomTable).not.toContain("vault_id");
  });

  it("uses maximum 3072D halfvec embeddings with HNSW indexes", () => {
    const sql = schemaStatements.join("\n").toLowerCase();
    expect(sql).toContain("vector halfvec(3072)");
    expect(sql).toContain("vector_subject halfvec(3072)");
    expect(sql).toContain("vector_content halfvec(3072)");
    expect(sql).toContain("halfvec_cosine_ops");
    expect(sql).toContain("using hnsw");
  });

  it("tracks embedding provenance for Atom and File vectors", () => {
    const sql = schemaStatements.join("\n").toLowerCase();
    expect(sql).toContain("vector_provider text");
    expect(sql).toContain("vector_model text");
    expect(sql).toContain("vector_dimensions integer");
    expect(sql).toContain("vector_embedded_at timestamptz");
    expect(sql).toContain("vector_subject_provider text");
    expect(sql).toContain("vector_content_provider text");
    expect(sql).toContain("atoms_vector_provenance_idx");
    expect(sql).toContain("files_vector_content_provenance_idx");
  });

  it("indexes metadata for structured and full-text retrieval", () => {
    const sql = schemaStatements.join("\n").toLowerCase();
    expect(sql).toContain("atoms_metadata_gin");
    expect(sql).toContain("atoms_metadata_fts_gin");
    expect(sql).toContain("files_metadata_gin");
    expect(sql).toContain("files_metadata_fts_gin");
  });

  it("invalidates legacy vectors that have no trustworthy provenance", () => {
    const sql = schemaStatements.join("\n").toLowerCase();
    expect(sql).toContain("foundation_vector_upgrade");
    expect(sql).toContain("vector_provider is null");
    expect(sql).toContain("vector_subject_provider is null");
    expect(sql).toContain("vector_content_provider is null");
  });

  it("includes external sync and OAuth state tables", () => {
    const sql = schemaStatements.join("\n").toLowerCase();
    expect(sql).toContain("create table if not exists external_bindings");
    expect(sql).toContain("create table if not exists oauth_clients");
    expect(sql).toContain("create table if not exists oauth_authorization_codes");
    expect(sql).toContain("create table if not exists oauth_tokens");
    expect(sql).toContain("unique (system, external_id)");
  });

  it("stores automatic remember reconciliation history", () => {
    const sql = schemaStatements.join("\n").toLowerCase();
    expect(sql).toContain("create table if not exists atom_relations");
    expect(sql).toContain("relation in ('supersedes','conflicts_with','refines','duplicate_of')");
    expect(sql).toContain("create table if not exists memory_requests");
    expect(sql).toContain("create table if not exists memory_events");
    expect(sql).toContain("action in ('new','duplicate','update','supersede','conflict')");
  });
});
