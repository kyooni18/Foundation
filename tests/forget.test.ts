import { describe, expect, it, vi } from "vitest";
import type { AIProvider } from "../src/ai.js";
import type { Database } from "../src/db.js";
import { Foundation } from "../src/foundation.js";

function foundationWithQuery(query: ReturnType<typeof vi.fn>): Foundation {
  const db = { query } as unknown as Database;
  const ai = {} as AIProvider;
  return new Foundation(db, ai, "UTC");
}

describe("Foundation forget", () => {
  const id = "11111111-1111-4111-8111-111111111111";

  it("deactivates an active memory and returns only compact status", async () => {
    const query = vi.fn().mockResolvedValueOnce({ rowCount: 1, rows: [{ id }] });
    const foundation = foundationWithQuery(query);

    await expect(foundation.forget(id)).resolves.toEqual({ id, forgotten: true });
    expect(query).toHaveBeenCalledTimes(1);
    expect(String(query.mock.calls[0]?.[0])).toContain("metadata=metadata || '{\"active\":false}'::jsonb");
  });

  it("is idempotent for an already-forgotten memory", async () => {
    const query = vi.fn()
      .mockResolvedValueOnce({ rowCount: 0, rows: [] })
      .mockResolvedValueOnce({ rowCount: 1, rows: [{ "?column?": 1 }] });
    const foundation = foundationWithQuery(query);

    await expect(foundation.forget(id)).resolves.toEqual({ id, forgotten: true });
    expect(query).toHaveBeenCalledTimes(2);
  });

  it("reports false for an unknown memory id", async () => {
    const query = vi.fn()
      .mockResolvedValueOnce({ rowCount: 0, rows: [] })
      .mockResolvedValueOnce({ rowCount: 0, rows: [] });
    const foundation = foundationWithQuery(query);

    await expect(foundation.forget(id)).resolves.toEqual({ id, forgotten: false });
  });
});
