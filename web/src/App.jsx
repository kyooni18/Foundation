import DOMPurify from "dompurify";
import { marked } from "marked";
import { startTransition, useDeferredValue, useEffect, useState, useTransition } from "react";

const API_BASE = (import.meta.env.VITE_API_BASE || "/api").replace(/\/$/, "");
const STORAGE_KEY = "foundation-web-state-v1";
const EMPTY_SOURCE_FORM = {
  source_uid: "",
  source_type: "note",
  label: "",
  locator: "",
  metadata: ""
};

const NAV_ITEMS = [
  { id: "overview", label: "Overview", eyebrow: "Health and routing" },
  { id: "keys", label: "Keys", eyebrow: "Bootstrap and verify" },
  { id: "search", label: "Search", eyebrow: "Vault + memory relevance" },
  { id: "atoms", label: "Atoms", eyebrow: "Add, delete, and search" },
  { id: "sources", label: "Sources", eyebrow: "Index provenance graph" },
  { id: "vaults", label: "Vaults", eyebrow: "Status, search, and upload" },
  { id: "editor", label: "Editor", eyebrow: "Browse and edit vault notes" }
];

function readStoredState() {
  if (typeof window === "undefined") {
    return { apiKey: "", vaultUID: "" };
  }

  try {
    const raw = window.localStorage.getItem(STORAGE_KEY);
    if (!raw) {
      return { apiKey: "", vaultUID: "" };
    }

    const parsed = JSON.parse(raw);
    return {
      apiKey: typeof parsed.apiKey === "string" ? parsed.apiKey : "",
      vaultUID: typeof parsed.vaultUID === "string" ? parsed.vaultUID : ""
    };
  } catch {
    return { apiKey: "", vaultUID: "" };
  }
}

function formatTime(isoString) {
  try {
    return new Intl.DateTimeFormat(undefined, {
      dateStyle: "medium",
      timeStyle: "short"
    }).format(new Date(isoString));
  } catch {
    return isoString;
  }
}

function formatNumber(value, maximumFractionDigits = 3) {
  if (typeof value !== "number" || Number.isNaN(value)) {
    return "n/a";
  }

  return new Intl.NumberFormat(undefined, {
    maximumFractionDigits
  }).format(value);
}

function decodeJsonIfPossible(value) {
  if (typeof value !== "string" || !value.trim()) {
    return value;
  }

  try {
    return JSON.parse(value);
  } catch {
    return value;
  }
}

function escapeRegExp(value) {
  return value.replace(/[.*+?^${}()|[\]\\]/g, "\\$&");
}

function splitQueryTerms(query) {
  return query
    .trim()
    .split(/\s+/)
    .map((term) => term.toLowerCase())
    .filter((term) => term.length >= 2);
}

function highlightText(text, terms) {
  if (!text) return "";
  if (!terms.length) return text;
  const pattern = terms.map(escapeRegExp).join("|");
  const regex = new RegExp(`(${pattern})`, "ig");
  const pieces = text.split(regex);
  return pieces.map((piece, index) =>
    terms.includes(piece.toLowerCase()) ? <mark key={`${piece}-${index}`}>{piece}</mark> : <span key={`${piece}-${index}`}>{piece}</span>
  );
}

function confidenceFromDistance(distance) {
  if (typeof distance !== "number" || Number.isNaN(distance)) {
    return { label: "Unknown", tone: "unknown" };
  }
  if (distance <= 0.2) {
    return { label: "High", tone: "high" };
  }
  if (distance <= 0.45) {
    return { label: "Medium", tone: "medium" };
  }
  return { label: "Low", tone: "low" };
}

function bytesToBase64(bytes) {
  let binary = "";
  const chunkSize = 0x8000;

  for (let idx = 0; idx < bytes.length; idx += chunkSize) {
    const chunk = bytes.subarray(idx, idx + chunkSize);
    binary += String.fromCharCode(...chunk);
  }

  return window.btoa(binary);
}

async function fileToBase64(file) {
  const buffer = await file.arrayBuffer();
  return bytesToBase64(new Uint8Array(buffer));
}

function base64ToText(base64) {
  if (!base64) return "";
  try {
    const binary = window.atob(base64.replace(/\s/g, ""));
    const bytes = Uint8Array.from(binary, (ch) => ch.charCodeAt(0));
    return new TextDecoder("utf-8").decode(bytes);
  } catch {
    return "";
  }
}

function textToBase64(text) {
  const bytes = new TextEncoder().encode(text);
  return bytesToBase64(bytes);
}

async function foundationFetch(path, { method = "GET", apiKey, payload, headers } = {}) {
  const requestHeaders = new Headers(headers || {});

  if (payload !== undefined) {
    requestHeaders.set("Content-Type", "application/json");
  }

  if (apiKey) {
    requestHeaders.set("Authorization", `Bearer ${apiKey}`);
  }

  const response = await fetch(`${API_BASE}${path}`, {
    method,
    headers: requestHeaders,
    body: payload === undefined ? undefined : JSON.stringify(payload)
  });

  const contentType = response.headers.get("content-type") || "";
  const data = contentType.includes("application/json")
    ? await response.json()
    : { ok: response.ok, raw: await response.text() };

  if (!response.ok) {
    const message = data?.error || data?.reason || data?.raw || `${response.status} ${response.statusText}`;
    throw new Error(message);
  }

  if (contentType.includes("application/json") && data && data.ok === false) {
    throw new Error(data.error || data.reason || "Request failed");
  }

  return data;
}

function AppShell({ activeTab, onTabChange, apiKeyPresent, children }) {
  return (
    <div className="app-shell">
      <aside className="sidebar">
        <div className="brand-block">
          <div className="brand-mark">F</div>
          <div>
            <p className="eyebrow">Foundation Web</p>
            <h1>Control room for memory, sources, and vault search.</h1>
          </div>
        </div>

        <p className="sidebar-copy">
          A React front end for the existing Foundation API, with browser-friendly auth storage and a same-origin
          proxy to the Swift server.
        </p>

        <div className="status-strip">
          <span className={`status-dot ${apiKeyPresent ? "online" : "idle"}`} />
          <span>{apiKeyPresent ? "API key loaded" : "No API key yet"}</span>
        </div>

        <nav className="nav-list" aria-label="Foundation sections">
          {NAV_ITEMS.map((item) => (
            <button
              key={item.id}
              type="button"
              className={`nav-item ${activeTab === item.id ? "active" : ""}`}
              onClick={() => onTabChange(item.id)}
            >
              <span className="nav-eyebrow">{item.eyebrow}</span>
              <span className="nav-label">{item.label}</span>
            </button>
          ))}
        </nav>
      </aside>

      <main className="workspace">{children}</main>
    </div>
  );
}

function Hero({ apiKey, onApiKeyChange, vaultUID, onVaultUIDChange, onRunHealthChecks }) {
  return (
    <section className="hero">
      <div className="hero-copy">
        <p className="eyebrow">Universal endpoint</p>
        <h2>Manage keys, search memory, inspect sources, and work with vaults from one web app.</h2>
        <p className="hero-detail">
          The browser talks to <code>{API_BASE}</code>, which is proxied inside Docker to the existing Swift
          Foundation server.
        </p>
      </div>

      <div className="hero-panel">
        <label className="field">
          <span>Foundation API key</span>
          <input
            type="password"
            value={apiKey}
            onChange={(event) => onApiKeyChange(event.target.value)}
            placeholder="foundation_..."
            autoComplete="off"
          />
        </label>

        <label className="field">
          <span>Default vault UID</span>
          <input
            type="text"
            value={vaultUID}
            onChange={(event) => onVaultUIDChange(event.target.value)}
            placeholder="archive"
          />
        </label>

        <div className="button-row">
          <button type="button" className="button primary" onClick={onRunHealthChecks}>
            Run health sweep
          </button>
          <a className="button ghost" href={`${API_BASE}/settings/login`} target="_blank" rel="noreferrer">
            Open native settings
          </a>
        </div>
      </div>
    </section>
  );
}

function Panel({ title, eyebrow, detail, actions, children }) {
  return (
    <section className="panel">
      <div className="panel-header">
        <div>
          {eyebrow ? <p className="eyebrow">{eyebrow}</p> : null}
          <h3>{title}</h3>
          {detail ? <p className="panel-detail">{detail}</p> : null}
        </div>
        {actions ? <div className="button-row">{actions}</div> : null}
      </div>
      {children}
    </section>
  );
}

function MetricRow({ items }) {
  return (
    <div className="metric-grid">
      {items.map((item) => (
        <div key={item.label} className="metric">
          <span>{item.label}</span>
          <strong>{item.value}</strong>
          {item.note ? <small>{item.note}</small> : null}
        </div>
      ))}
    </div>
  );
}

function Feed({ items }) {
  return (
    <div className="feed">
      {items.length === 0 ? (
        <p className="empty-state">No activity yet. Run one of the actions in the workspace to populate the feed.</p>
      ) : (
        items.map((item) => (
          <article key={item.id} className={`feed-item ${item.kind}`}>
            <header>
              <strong>{item.title}</strong>
              <span>{formatTime(item.createdAt)}</span>
            </header>
            <p>{item.detail}</p>
          </article>
        ))
      )}
    </div>
  );
}

function JsonView({ title, value }) {
  if (!value) {
    return <p className="empty-state">No response captured yet.</p>;
  }

  return (
    <div className="json-block">
      {title ? <p className="eyebrow">{title}</p> : null}
      <pre>{JSON.stringify(value, null, 2)}</pre>
    </div>
  );
}

function ResultTable({ columns, rows, emptyText = "No rows yet." }) {
  if (!rows.length) {
    return <p className="empty-state">{emptyText}</p>;
  }

  return (
    <div className="table-wrap">
      <table>
        <thead>
          <tr>
            {columns.map((column) => (
              <th key={column.key}>{column.label}</th>
            ))}
          </tr>
        </thead>
        <tbody>
          {rows.map((row, rowIndex) => (
            <tr key={row.id || row.source_uid || row.file_path || row.change_id || rowIndex}>
              {columns.map((column) => (
                <td key={column.key}>{column.render ? column.render(row[column.key], row) : String(row[column.key] ?? "")}</td>
              ))}
            </tr>
          ))}
        </tbody>
      </table>
    </div>
  );
}

function App() {
  const stored = readStoredState();
  const [activeTab, setActiveTab] = useState("overview");
  const [apiKey, setApiKey] = useState(stored.apiKey);
  const [vaultUID, setVaultUID] = useState(stored.vaultUID);
  const [healthState, setHealthState] = useState({
    service: null,
    database: null,
    embed: null
  });
  const [lastResponse, setLastResponse] = useState(null);
  const [feedItems, setFeedItems] = useState([]);
  const [sourceList, setSourceList] = useState([]);
  const [sourceSimilar, setSourceSimilar] = useState([]);
  const [sourceFilter, setSourceFilter] = useState("");
  const [sourceForm, setSourceForm] = useState(EMPTY_SOURCE_FORM);
  const [selectedSourceUID, setSelectedSourceUID] = useState("");
  const [sourceAtomText, setSourceAtomText] = useState("");
  const [sourceSimilarLimit, setSourceSimilarLimit] = useState(5);
  const [atomText, setAtomText] = useState("");
  const [findText, setFindText] = useState("");
  const [deleteText, setDeleteText] = useState("");
  const [searchQuery, setSearchQuery] = useState("");
  const [debouncedSearchQuery, setDebouncedSearchQuery] = useState("");
  const [includeMemoryMatches, setIncludeMemoryMatches] = useState(true);
  const [searchLoading, setSearchLoading] = useState(false);
  const [searchError, setSearchError] = useState("");
  const [lastSearchedAt, setLastSearchedAt] = useState(null);
  const [searchResults, setSearchResults] = useState({ vault: [], memory: [] });
  const [embedInput, setEmbedInput] = useState("");
  const [atomResults, setAtomResults] = useState([]);
  const [embedResult, setEmbedResult] = useState(null);
  const [listKeysResult, setListKeysResult] = useState("");
  const [masterKey, setMasterKey] = useState("");
  const [verifyKey, setVerifyKey] = useState("");
  const [deleteKey, setDeleteKey] = useState("");
  const [createdKeyResult, setCreatedKeyResult] = useState(null);
  const [verifyResult, setVerifyResult] = useState(null);
  const [vaultQuery, setVaultQuery] = useState("");
  const [vaultStatus, setVaultStatus] = useState(null);
  const [vaultSearchResults, setVaultSearchResults] = useState([]);
  const [vaultSnapshot, setVaultSnapshot] = useState([]);
  const [uploadFiles, setUploadFiles] = useState([]);
  const [uploadSummary, setUploadSummary] = useState(null);
  const [editorFiles, setEditorFiles] = useState([]);
  const [editorFileFilter, setEditorFileFilter] = useState("");
  const [editorSelectedPath, setEditorSelectedPath] = useState(null);
  const [editorContent, setEditorContent] = useState("");
  const [editorPreviewMode, setEditorPreviewMode] = useState(false);
  const [editorSaving, setEditorSaving] = useState(false);
  const [isPending, startUiTransition] = useTransition();
  const deferredSourceFilter = useDeferredValue(sourceFilter);

  useEffect(() => {
    window.localStorage.setItem(
      STORAGE_KEY,
      JSON.stringify({
        apiKey,
        vaultUID
      })
    );
  }, [apiKey, vaultUID]);

  useEffect(() => {
    const timer = window.setTimeout(() => {
      setDebouncedSearchQuery(searchQuery);
    }, 300);

    return () => window.clearTimeout(timer);
  }, [searchQuery]);

  const filteredSources = sourceList.filter((source) => {
    const query = deferredSourceFilter.trim().toLowerCase();
    if (!query) {
      return true;
    }

    const haystack = [
      source.source_uid,
      source.source_type,
      source.label,
      source.locator,
      source.metadata
    ]
      .filter(Boolean)
      .join(" ")
      .toLowerCase();

    return haystack.includes(query);
  });

  function pushFeed(title, detail, kind = "info") {
    setFeedItems((current) => [
      {
        id: `${Date.now()}-${Math.random().toString(16).slice(2)}`,
        title,
        detail,
        kind,
        createdAt: new Date().toISOString()
      },
      ...current
    ].slice(0, 12));
  }

  async function runAction({
    title,
    path,
    method = "GET",
    payload,
    auth = false,
    onSuccess,
    successMessage
  }) {
    try {
      const data = await foundationFetch(path, {
        method,
        payload,
        apiKey: auth ? apiKey : undefined
      });

      setLastResponse(data);
      if (onSuccess) {
        startUiTransition(() => {
          onSuccess(data);
        });
      }

      pushFeed(title, successMessage || "Action completed successfully.", "success");
      return data;
    } catch (error) {
      setLastResponse({
        ok: false,
        error: error.message
      });
      pushFeed(title, error.message, "error");
      return null;
    }
  }

  async function runHealthChecks() {
    const [service, database, embed] = await Promise.all([
      runAction({
        title: "Service health",
        path: "/health",
        successMessage: "Foundation service responded."
      }),
      runAction({
        title: "Database health",
        path: "/health/db",
        successMessage: "Database route responded."
      }),
      runAction({
        title: "Embedding health",
        path: "/health/embed",
        successMessage: "Embedding provider responded."
      })
    ]);

    startTransition(() => {
      setHealthState({
        service,
        database,
        embed
      });
    });
  }

  async function loadSources() {
    await runAction({
      title: "Load sources",
      path: "/sources/list",
      auth: true,
      onSuccess: (data) => {
        setSourceList(data.results || []);
        if (!selectedSourceUID && data.results?.[0]?.source_uid) {
          setSelectedSourceUID(data.results[0].source_uid);
        }
      },
      successMessage: "Source list refreshed."
    });
  }

  async function createSource() {
    const payload = {
      source_type: sourceForm.source_type,
      label: sourceForm.label,
      locator: sourceForm.locator,
      metadata: sourceForm.metadata
    };

    if (sourceForm.source_uid.trim()) {
      payload.source_uid = sourceForm.source_uid.trim();
    }

    const data = await runAction({
      title: "Create source",
      path: "/sources/create",
      method: "POST",
      payload,
      auth: true,
      successMessage: "Source upsert completed."
    });

    if (data?.ok && data.source_uid) {
      setSelectedSourceUID(data.source_uid);
      setSourceForm(EMPTY_SOURCE_FORM);
      await loadSources();
    }
  }

  async function linkAtomToSource() {
    if (!selectedSourceUID || !sourceAtomText.trim()) {
      pushFeed("Link source atom", "Select a source and provide an atom text first.", "error");
      return;
    }

    await runAction({
      title: "Link source atom",
      path: "/sources/link-atom",
      method: "POST",
      auth: true,
      payload: {
        source_uid: selectedSourceUID,
        atom_text: sourceAtomText.trim()
      },
      successMessage: `Linked atom text to ${selectedSourceUID}.`
    });
  }

  async function refreshSimilar(linkResults) {
    if (!selectedSourceUID) {
      pushFeed(linkResults ? "Persist similar sources" : "Find similar sources", "Choose a source first.", "error");
      return;
    }

    await runAction({
      title: linkResults ? "Persist similar sources" : "Find similar sources",
      path: linkResults ? "/sources/link-similar" : "/sources/find-similar",
      method: "POST",
      auth: true,
      payload: {
        source_uid: selectedSourceUID,
        limit: Number(sourceSimilarLimit) || 5
      },
      onSuccess: (data) => {
        setSourceSimilar(data.results || []);
      },
      successMessage: linkResults ? "Persisted similarity edges." : "Computed nearest sources."
    });
  }

  async function reindexSource() {
    if (!selectedSourceUID) {
      pushFeed("Reindex source", "Choose a source first.", "error");
      return;
    }

    await runAction({
      title: "Reindex source",
      path: "/sources/reindex",
      method: "POST",
      auth: true,
      payload: {
        source_uid: selectedSourceUID
      },
      successMessage: `Reindexed ${selectedSourceUID}.`
    });
  }

  async function listKeys() {
    await runAction({
      title: "List keys",
      path: "/keys/list",
      onSuccess: (data) => {
        setListKeysResult(data.result || "");
      },
      successMessage: "Loaded masked key list."
    });
  }

  async function createKey() {
    const data = await runAction({
      title: "Create key",
      path: "/keys/create",
      method: "POST",
      payload: {
        api_key: masterKey
      },
      onSuccess: (response) => {
        setCreatedKeyResult(response);
      },
      successMessage: "Created a new API key."
    });

    if (data?.api_key) {
      setApiKey(data.api_key);
    }
  }

  async function verifyCurrentKey() {
    await runAction({
      title: "Verify key",
      path: "/keys/verify",
      method: "POST",
      payload: {
        api_key: verifyKey
      },
      onSuccess: (data) => {
        setVerifyResult(data);
      },
      successMessage: "Verification completed."
    });
  }

  async function deleteCurrentKey() {
    await runAction({
      title: "Delete key",
      path: "/keys/delete",
      method: "POST",
      payload: {
        api_key: deleteKey
      },
      successMessage: "Delete request completed."
    });
  }

  async function addAtom() {
    await runAction({
      title: "Add atom",
      path: "/add",
      method: "POST",
      payload: { text: atomText },
      auth: true,
      successMessage: "Atom inserted."
    });
  }

  async function deleteAtom() {
    await runAction({
      title: "Delete atom",
      path: "/delete",
      method: "POST",
      payload: { text: deleteText },
      auth: true,
      successMessage: "Delete request completed."
    });
  }

  async function findAtoms(queryOverride) {
    const text = (queryOverride ?? findText).trim();
    const data = await runAction({
      title: "Find atoms",
      path: "/find",
      method: "POST",
      payload: { text },
      auth: true,
      onSuccess: (data) => {
        if (queryOverride === undefined) {
          setAtomResults(data.results || []);
        }
      },
      successMessage: "Nearest atoms loaded."
    });
    return data?.results || [];
  }

  async function generateEmbedding() {
    await runAction({
      title: "Embed text",
      path: "/embed/text",
      method: "POST",
      payload: { text: embedInput },
      auth: true,
      onSuccess: (data) => {
        setEmbedResult({
          size: data.embedding?.length || 0,
          preview: data.embedding?.slice(0, 8) || []
        });
      },
      successMessage: "Embedding generated."
    });
  }

  async function loadVaultStatus() {
    if (!vaultUID.trim()) {
      pushFeed("Load vault status", "Enter a vault UID first.", "error");
      return;
    }

    await runAction({
      title: "Load vault status",
      path: "/vaults/sync/status",
      method: "POST",
      auth: true,
      payload: {
        vault_uid: vaultUID,
        limit: 100
      },
      onSuccess: (data) => {
        setVaultStatus(data);
      },
      successMessage: `Loaded vault status for ${vaultUID}.`
    });
  }

  async function searchVault(queryOverride) {
    if (!vaultUID.trim()) {
      pushFeed("Search vault", "Enter a vault UID first.", "error");
      return [];
    }

    const query = queryOverride ?? vaultQuery;
    const data = await runAction({
      title: "Search vault",
      path: "/vaults/search",
      method: "POST",
      auth: true,
      payload: {
        vault_uid: vaultUID,
        query,
        limit: 8
      },
      onSuccess: (data) => {
        if (queryOverride === undefined) {
          setVaultSearchResults(data.results || []);
        }
      },
      successMessage: `Ran semantic vault search for ${vaultUID}.`
    });
    return data?.results || [];
  }

  async function runUnifiedSearch(manualQuery) {
    const query = (manualQuery ?? debouncedSearchQuery).trim();
    if (!vaultUID.trim()) {
      setSearchError("Set a vault UID before searching.");
      setSearchResults({ vault: [], memory: [] });
      return;
    }
    if (query.length < 2) {
      setSearchError("");
      setSearchResults({ vault: [], memory: [] });
      return;
    }

    setSearchLoading(true);
    setSearchError("");

    try {
      const [vaultMatches, memoryMatches] = await Promise.all([
        searchVault(query),
        includeMemoryMatches ? findAtoms(query) : Promise.resolve([])
      ]);

      setSearchResults({
        vault: [...vaultMatches].sort((a, b) => (a.distance ?? Number.POSITIVE_INFINITY) - (b.distance ?? Number.POSITIVE_INFINITY)),
        memory: [...memoryMatches].sort((a, b) => (a.distance ?? Number.POSITIVE_INFINITY) - (b.distance ?? Number.POSITIVE_INFINITY))
      });
      setLastSearchedAt(new Date().toISOString());
    } catch (error) {
      setSearchError(error.message || "Search failed.");
    } finally {
      setSearchLoading(false);
    }
  }

  async function loadVaultSnapshot() {
    if (!vaultUID.trim()) {
      pushFeed("Load vault snapshot", "Enter a vault UID first.", "error");
      return;
    }

    await runAction({
      title: "Load vault snapshot",
      path: "/vaults/sync/full-pull",
      method: "POST",
      auth: true,
      payload: {
        vault_uid: vaultUID
      },
      onSuccess: (data) => {
        setVaultSnapshot(data.snapshot_files || []);
      },
      successMessage: `Loaded full snapshot for ${vaultUID}.`
    });
  }

  async function uploadVaultFiles() {
    if (!vaultUID.trim()) {
      pushFeed("Upload vault snapshot", "Enter a vault UID first.", "error");
      return;
    }

    if (uploadFiles.length === 0) {
      pushFeed("Upload vault snapshot", "Choose a folder or files first.", "error");
      return;
    }

    const files = await Promise.all(
      uploadFiles.map(async (file) => ({
        file_path: file.webkitRelativePath || file.name,
        content_base64: await fileToBase64(file)
      }))
    );

    await runAction({
      title: "Upload vault snapshot",
      path: "/vaults/sync/full-push",
      method: "POST",
      auth: true,
      payload: {
        vault_uid: vaultUID,
        device_id: "foundation-web",
        uploaded_at_unix_ms: Date.now(),
        files
      },
      onSuccess: (data) => {
        setUploadSummary({
          uploaded: files.length,
          response: data
        });
      },
      successMessage: `Uploaded ${files.length} files to ${vaultUID}.`
    });
  }

  async function loadEditorFiles() {
    if (!vaultUID.trim()) {
      pushFeed("Load vault files", "Enter a vault UID first.", "error");
      return;
    }

    await runAction({
      title: "Load vault files",
      path: "/vaults/sync/full-pull",
      method: "POST",
      auth: true,
      payload: { vault_uid: vaultUID },
      onSuccess: (data) => {
        const files = (data.snapshot_files || []).filter((f) => f.file_path.endsWith(".md") || f.file_path.endsWith(".txt"));
        setEditorFiles(files);
        if (files.length > 0 && !editorSelectedPath) {
          const first = files[0];
          setEditorSelectedPath(first.file_path);
          setEditorContent(base64ToText(first.content_base64 || ""));
          setEditorPreviewMode(false);
        }
      },
      successMessage: `Loaded ${vaultUID} file list for editor.`
    });
  }

  function openEditorFile(file) {
    setEditorSelectedPath(file.file_path);
    setEditorContent(base64ToText(file.content_base64 || ""));
    setEditorPreviewMode(false);
  }

  async function saveEditorFile() {
    if (!vaultUID.trim() || !editorSelectedPath) {
      pushFeed("Save file", "Open a vault file first.", "error");
      return;
    }

    setEditorSaving(true);

    const contentBase64 = textToBase64(editorContent);
    const sizeBytes = new TextEncoder().encode(editorContent).length;

    await runAction({
      title: "Save file",
      path: "/vaults/sync/push",
      method: "POST",
      auth: true,
      payload: {
        vault_uid: vaultUID,
        device_id: "foundation-web-editor",
        changes: [
          {
            file_path: editorSelectedPath,
            action: "modified",
            changed_at_unix_ms: Date.now(),
            content_base64: contentBase64,
            size_bytes: sizeBytes
          }
        ]
      },
      onSuccess: () => {
        setEditorFiles((prev) =>
          prev.map((f) =>
            f.file_path === editorSelectedPath
              ? { ...f, content_base64: contentBase64, size_bytes: sizeBytes }
              : f
          )
        );
      },
      successMessage: `Saved ${editorSelectedPath} to ${vaultUID}.`
    });

    setEditorSaving(false);
  }

  const healthMetrics = [
    {
      label: "Service",
      value: healthState.service?.ok ? "online" : "idle",
      note: healthState.service ? "HTTP /health" : "not checked"
    },
    {
      label: "Database",
      value: healthState.database?.db || "unknown",
      note: healthState.database ? "first visible table" : "not checked"
    },
    {
      label: "Embedding",
      value: healthState.embed?.embed_dim ? `${healthState.embed.embed_dim} dims` : "not checked",
      note: "runtime embedding probe"
    }
  ];

  const selectedSource = sourceList.find((item) => item.source_uid === selectedSourceUID);
  const queryTerms = splitQueryTerms(searchQuery);
  const vaultMatches = searchResults.vault;
  const memoryMatches = searchResults.memory;

  useEffect(() => {
    if (debouncedSearchQuery.trim().length >= 2) {
      runUnifiedSearch(debouncedSearchQuery);
    }
  }, [debouncedSearchQuery, includeMemoryMatches]);

  function submitSearch(event) {
    event.preventDefault();
    runUnifiedSearch(searchQuery);
  }

  function openVaultResultInEditor(match) {
    const cached = editorFiles.find((file) => file.file_path === match.file_path);
    if (!cached) {
      pushFeed("Open vault match", "File is not loaded in editor cache yet. Use Editor > Load files first.", "error");
      return;
    }

    openEditorFile(cached);
    setActiveTab("editor");
    pushFeed("Open vault match", `Opened ${match.file_path} in Editor tab.`, "success");
  }

  return (
    <AppShell activeTab={activeTab} onTabChange={setActiveTab} apiKeyPresent={Boolean(apiKey)}>
      <Hero
        apiKey={apiKey}
        onApiKeyChange={setApiKey}
        vaultUID={vaultUID}
        onVaultUIDChange={setVaultUID}
        onRunHealthChecks={runHealthChecks}
      />

      <div className="content-grid">
        <div className="primary-column">
          {activeTab === "overview" ? (
            <>
              <Panel
                title="Runtime sweep"
                eyebrow="Foundation stack"
                detail="Quick checks against service, database, and embedding routes."
                actions={
                  <button type="button" className="button primary" onClick={runHealthChecks}>
                    Refresh health
                  </button>
                }
              >
                <MetricRow items={healthMetrics} />
              </Panel>

              <Panel
                title="Response inspector"
                eyebrow="Last API payload"
                detail="Useful while you are validating new endpoints or seeing the exact server response."
              >
                <JsonView value={lastResponse} />
              </Panel>
            </>
          ) : null}

          {activeTab === "keys" ? (
            <>
              <Panel
                title="Key lifecycle"
                eyebrow="Public endpoints"
                detail="Create operational keys from the master key, verify them, and inspect masked entries."
                actions={
                  <button type="button" className="button primary" onClick={listKeys}>
                    List keys
                  </button>
                }
              >
                <div className="split-grid">
                  <label className="field">
                    <span>Master key</span>
                    <input type="password" value={masterKey} onChange={(event) => setMasterKey(event.target.value)} />
                  </label>
                  <button type="button" className="button primary" onClick={createKey}>
                    Create API key
                  </button>
                  <label className="field">
                    <span>Verify key</span>
                    <input type="text" value={verifyKey} onChange={(event) => setVerifyKey(event.target.value)} />
                  </label>
                  <button type="button" className="button ghost" onClick={verifyCurrentKey}>
                    Verify
                  </button>
                  <label className="field">
                    <span>Delete key</span>
                    <input type="text" value={deleteKey} onChange={(event) => setDeleteKey(event.target.value)} />
                  </label>
                  <button type="button" className="button ghost danger" onClick={deleteCurrentKey}>
                    Delete
                  </button>
                </div>

                <MetricRow
                  items={[
                    { label: "Created key", value: createdKeyResult?.mask || "none yet" },
                    { label: "Verify result", value: verifyResult ? String(verifyResult.valid) : "not run" },
                    { label: "Loaded key count", value: listKeysResult ? listKeysResult.trim().split("\n").length : "0" }
                  ]}
                />

                <JsonView title="Masked key list" value={listKeysResult ? { result: listKeysResult } : createdKeyResult} />
              </Panel>
            </>
          ) : null}

          {activeTab === "search" ? (
            <>
              <Panel
                title="Unified search"
                eyebrow="Search-first flow"
                detail="Semantic vault matches are primary; nearest memory atoms are optional and run in parallel."
              >
                <form className="search-shell" onSubmit={submitSearch}>
                  <div className="search-query-bar">
                    <label className="field grow">
                      <span>Search query</span>
                      <input
                        type="text"
                        value={searchQuery}
                        onChange={(event) => setSearchQuery(event.target.value)}
                        placeholder="Ask across vault notes and memory atoms…"
                      />
                    </label>
                    <label className="toggle">
                      <input
                        type="checkbox"
                        checked={includeMemoryMatches}
                        onChange={(event) => setIncludeMemoryMatches(event.target.checked)}
                      />
                      Include memory matches
                    </label>
                    <button type="submit" className="button primary" disabled={searchLoading}>
                      {searchLoading ? "Searching…" : "Search"}
                    </button>
                  </div>

                  {lastSearchedAt ? <p className="subtle-note">Last searched: {formatTime(lastSearchedAt)}</p> : null}
                  {searchError ? <p className="search-error">{searchError}</p> : null}

                  {!searchQuery.trim() ? (
                    <p className="empty-state">Start typing a focused query (concept, person, or file topic), then press Search.</p>
                  ) : null}
                  {searchQuery.trim() && searchQuery.trim().length < 2 ? (
                    <p className="empty-state">Use at least 2 characters so semantic ranking can produce stable matches.</p>
                  ) : null}
                </form>
              </Panel>

              <Panel title="Vault matches" eyebrow="Primary source">
                {vaultMatches.length === 0 ? (
                  <p className="empty-state">No vault matches yet. Try a narrower topic, or verify the selected vault has indexed notes.</p>
                ) : (
                  <div className="search-results-grid">
                    {vaultMatches.map((match, index) => {
                      const confidence = confidenceFromDistance(match.distance);
                      return (
                        <article key={`${match.file_path}-${index}`} className="search-card">
                          <header>
                            <button type="button" className="search-result-link" onClick={() => openVaultResultInEditor(match)}>
                              {match.file_path}
                            </button>
                            <span className={`confidence-badge ${confidence.tone}`}>{confidence.label}</span>
                          </header>
                          <p className="search-title">{highlightText(match.title || "Untitled", queryTerms)}</p>
                          <p className="search-snippet">{highlightText(match.keypoint || "No keypoint snippet available.", queryTerms)}</p>
                          <p className="search-distance">Distance: {formatNumber(match.distance, 5)}</p>
                        </article>
                      );
                    })}
                  </div>
                )}
              </Panel>

              <Panel title="Memory matches" eyebrow="Optional secondary source">
                {memoryMatches.length === 0 ? (
                  <p className="empty-state">
                    {includeMemoryMatches
                      ? "No nearby memory atoms yet. Add atoms in the Atoms tab to enrich this source."
                      : "Memory matching is disabled. Enable “Include memory matches” to compare atom neighbors."}
                  </p>
                ) : (
                  <div className="search-results-grid">
                    {memoryMatches.map((match, index) => {
                      const confidence = confidenceFromDistance(match.distance);
                      const decoded = decodeJsonIfPossible(match.metadata);
                      return (
                        <article key={`${match.id || index}`} className="search-card compact">
                          <header>
                            <strong>Atom #{match.id ?? index + 1}</strong>
                            <span className={`confidence-badge ${confidence.tone}`}>{confidence.label}</span>
                          </header>
                          <p className="search-snippet">{highlightText(match.text || "", queryTerms)}</p>
                          <p className="search-distance">Distance: {formatNumber(match.distance, 5)}</p>
                          <p className="search-meta">{typeof decoded === "string" ? decoded : JSON.stringify(decoded)}</p>
                        </article>
                      );
                    })}
                  </div>
                )}
              </Panel>
            </>
          ) : null}

          {activeTab === "atoms" ? (
            <>
              <Panel title="Memory controls" eyebrow="Protected endpoints" detail="Add atoms, run nearest-neighbor search, and delete exact text matches.">
                <div className="action-cluster">
                  <div className="form-panel">
                    <label className="field">
                      <span>Add atom</span>
                      <textarea value={atomText} onChange={(event) => setAtomText(event.target.value)} rows={4} />
                    </label>
                    <button type="button" className="button primary" onClick={addAtom}>
                      Insert text
                    </button>
                  </div>

                  <div className="form-panel">
                    <label className="field">
                      <span>Find nearby atoms</span>
                      <textarea value={findText} onChange={(event) => setFindText(event.target.value)} rows={4} />
                    </label>
                    <button type="button" className="button primary" onClick={findAtoms}>
                      Search memory
                    </button>
                  </div>

                  <div className="form-panel">
                    <label className="field">
                      <span>Delete exact text</span>
                      <textarea value={deleteText} onChange={(event) => setDeleteText(event.target.value)} rows={4} />
                    </label>
                    <button type="button" className="button ghost danger" onClick={deleteAtom}>
                      Delete text
                    </button>
                  </div>
                </div>
              </Panel>

              <Panel title="Embedding probe" eyebrow="Vector preview" detail="Quick sanity check for the active embedding provider and dimensionality.">
                <div className="inline-form">
                    <label className="field grow">
                      <span>Text to embed</span>
                    <input type="text" value={embedInput} onChange={(event) => setEmbedInput(event.target.value)} />
                    </label>
                  <button type="button" className="button ghost" onClick={generateEmbedding}>
                    Generate embedding
                  </button>
                </div>
                <MetricRow
                  items={[
                    { label: "Vector size", value: embedResult?.size || "n/a" },
                    {
                      label: "Preview",
                      value: embedResult?.preview?.length ? embedResult.preview.map((item) => formatNumber(item)).join(", ") : "n/a"
                    }
                  ]}
                />
              </Panel>

              <Panel title="Nearest atoms" eyebrow="Search results">
                <ResultTable
                  rows={atomResults}
                  columns={[
                    { key: "id", label: "ID" },
                    { key: "text", label: "Text" },
                    { key: "distance", label: "Distance", render: (value) => formatNumber(value, 5) },
                    {
                      key: "metadata",
                      label: "Metadata",
                      render: (value) => {
                        const decoded = decodeJsonIfPossible(value);
                        return typeof decoded === "string" ? decoded : JSON.stringify(decoded);
                      }
                    }
                  ]}
                  emptyText="Run a memory search to see nearest atoms here."
                />
              </Panel>
            </>
          ) : null}

          {activeTab === "sources" ? (
            <>
              <Panel
                title="Source graph"
                eyebrow="Create and inspect"
                detail="Upsert sources, filter them locally, and use the selected source for reindex and similarity actions."
                actions={
                  <button type="button" className="button primary" onClick={loadSources}>
                    Refresh source list
                  </button>
                }
              >
                <div className="split-grid">
                  <label className="field">
                    <span>Source UID</span>
                    <input
                      type="text"
                      value={sourceForm.source_uid}
                      onChange={(event) => setSourceForm((current) => ({ ...current, source_uid: event.target.value }))}
                    />
                  </label>
                  <label className="field">
                    <span>Source type</span>
                    <input
                      type="text"
                      value={sourceForm.source_type}
                      onChange={(event) => setSourceForm((current) => ({ ...current, source_type: event.target.value }))}
                    />
                  </label>
                  <label className="field">
                    <span>Label</span>
                    <input
                      type="text"
                      value={sourceForm.label}
                      onChange={(event) => setSourceForm((current) => ({ ...current, label: event.target.value }))}
                    />
                  </label>
                  <label className="field">
                    <span>Locator</span>
                    <input
                      type="text"
                      value={sourceForm.locator}
                      onChange={(event) => setSourceForm((current) => ({ ...current, locator: event.target.value }))}
                    />
                  </label>
                </div>
                <label className="field">
                  <span>Metadata JSON string</span>
                  <textarea
                    value={sourceForm.metadata}
                    onChange={(event) => setSourceForm((current) => ({ ...current, metadata: event.target.value }))}
                    rows={3}
                    placeholder='{"project":"foundation"}'
                  />
                </label>
                <div className="button-row">
                  <button type="button" className="button primary" onClick={createSource}>
                    Save source
                  </button>
                  <button type="button" className="button ghost" onClick={() => setSourceForm(EMPTY_SOURCE_FORM)}>
                    Reset form
                  </button>
                </div>
              </Panel>

              <Panel title="Selected source actions" eyebrow="Reindex and relate" detail="Choose a source, link an atom text to it, and compute or persist similarity edges.">
                <div className="split-grid">
                  <label className="field">
                    <span>Selected source</span>
                    <select value={selectedSourceUID} onChange={(event) => setSelectedSourceUID(event.target.value)}>
                      <option value="">Choose a source</option>
                      {sourceList.map((source) => (
                        <option key={source.source_uid} value={source.source_uid}>
                          {source.source_uid}
                        </option>
                      ))}
                    </select>
                  </label>

                  <label className="field">
                    <span>Similarity limit</span>
                    <input
                      type="number"
                      min="1"
                      max="50"
                      value={sourceSimilarLimit}
                      onChange={(event) => setSourceSimilarLimit(event.target.value)}
                    />
                  </label>

                  <label className="field full-span">
                    <span>Atom text to link</span>
                    <input type="text" value={sourceAtomText} onChange={(event) => setSourceAtomText(event.target.value)} />
                  </label>
                </div>

                <div className="button-row">
                  <button type="button" className="button primary" onClick={linkAtomToSource}>
                    Link atom text
                  </button>
                  <button type="button" className="button ghost" onClick={reindexSource}>
                    Reindex
                  </button>
                  <button type="button" className="button ghost" onClick={() => refreshSimilar(false)}>
                    Find similar
                  </button>
                  <button type="button" className="button ghost" onClick={() => refreshSimilar(true)}>
                    Persist links
                  </button>
                </div>

                {selectedSource ? (
                  <MetricRow
                    items={[
                      { label: "Selected", value: selectedSource.source_uid },
                      { label: "Type", value: selectedSource.source_type },
                      { label: "Linked atoms", value: selectedSource.linked_atom_count },
                      { label: "Indexed atoms", value: selectedSource.indexed_atom_count }
                    ]}
                  />
                ) : null}
              </Panel>

              <Panel title="Source inventory" eyebrow="Client-side filter" detail="The server returns up to 500 sources, then the browser filters locally for fast narrowing.">
                <label className="field">
                  <span>Filter sources</span>
                  <input type="text" value={sourceFilter} onChange={(event) => setSourceFilter(event.target.value)} />
                </label>
                <ResultTable
                  rows={filteredSources}
                  columns={[
                    { key: "source_uid", label: "UID" },
                    { key: "source_type", label: "Type" },
                    { key: "label", label: "Label" },
                    { key: "linked_atom_count", label: "Linked" },
                    { key: "indexed_atom_count", label: "Indexed" }
                  ]}
                  emptyText="No sources loaded yet."
                />
              </Panel>

              <Panel title="Similarity results" eyebrow="Source-to-source distance">
                <ResultTable
                  rows={sourceSimilar}
                  columns={[
                    { key: "source_uid", label: "UID" },
                    { key: "source_type", label: "Type" },
                    { key: "label", label: "Label" },
                    { key: "distance", label: "Distance", render: (value) => formatNumber(value, 5) }
                  ]}
                  emptyText="Run find similar or persist links to populate this table."
                />
              </Panel>
            </>
          ) : null}

          {activeTab === "vaults" ? (
            <>
              <Panel title="Vault operations" eyebrow="Search and status" detail="Use the stored vault UID for server status, semantic search, and whole-vault upload/pull workflows.">
                <div className="inline-form">
                  <label className="field grow">
                    <span>Vault query</span>
                    <input type="text" value={vaultQuery} onChange={(event) => setVaultQuery(event.target.value)} />
                  </label>
                  <button type="button" className="button primary" onClick={searchVault}>
                    Search vault
                  </button>
                  <button type="button" className="button ghost" onClick={loadVaultStatus}>
                    Load status
                  </button>
                  <button type="button" className="button ghost" onClick={loadVaultSnapshot}>
                    Full pull
                  </button>
                </div>

                {vaultStatus ? (
                  <MetricRow
                    items={[
                      { label: "Vault", value: vaultStatus.vault_uid },
                      { label: "Latest change ID", value: vaultStatus.latest_change_id || "n/a" },
                      { label: "Tracked files", value: vaultStatus.file_timestamps?.length || 0 },
                      { label: "Change log rows", value: vaultStatus.change_log?.length || 0 }
                    ]}
                  />
                ) : null}
              </Panel>

              <Panel title="Whole-vault upload" eyebrow="Browser file input" detail="Pick a folder or a batch of files and the app will stream them as a Foundation full-push payload.">
                <label className="field">
                  <span>Vault folder or files</span>
                  <input
                    type="file"
                    multiple
                    webkitdirectory="true"
                    onChange={(event) => setUploadFiles(Array.from(event.target.files || []))}
                  />
                </label>
                <div className="button-row">
                  <button type="button" className="button primary" onClick={uploadVaultFiles}>
                    Upload snapshot
                  </button>
                  <span className="subtle-note">{uploadFiles.length} files selected</span>
                </div>
                {uploadSummary ? <JsonView value={uploadSummary} /> : null}
              </Panel>

              <Panel title="Vault search results" eyebrow="Semantic matches">
                <ResultTable
                  rows={vaultSearchResults}
                  columns={[
                    { key: "file_path", label: "File" },
                    { key: "title", label: "Title" },
                    { key: "keypoint", label: "Keypoint" },
                    { key: "distance", label: "Distance", render: (value) => formatNumber(value, 5) },
                    { key: "obsidian_link", label: "Obsidian link" }
                  ]}
                  emptyText="Run a vault search to see semantic matches."
                />
              </Panel>

              <Panel title="Snapshot files" eyebrow="Full pull output" detail="A browser-side view of the most recent full-pull response.">
                <ResultTable
                  rows={vaultSnapshot}
                  columns={[
                    { key: "file_path", label: "File" },
                    { key: "size_bytes", label: "Bytes", render: (value) => formatNumber(value, 0) },
                    { key: "updated_unix_ms", label: "Updated", render: (value) => formatNumber(value, 0) }
                  ]}
                  emptyText="Run full pull to inspect snapshot files."
                />
              </Panel>
            </>
          ) : null}

          {activeTab === "editor" ? (
            <>
              <Panel
                title="Vault note editor"
                eyebrow="Browse and edit markdown"
                detail="Pull vault files from the server, pick a note, and edit or preview it. Saving pushes the change back via the sync API."
                actions={
                  <div className="button-row">
                    <button type="button" className="button primary" onClick={loadEditorFiles}>
                      Load files
                    </button>
                  </div>
                }
              >
                <div className="editor-layout">
                  <div className="editor-sidebar">
                    <label className="field">
                      <span>Filter files</span>
                      <input
                        type="text"
                        value={editorFileFilter}
                        onChange={(event) => setEditorFileFilter(event.target.value)}
                        placeholder="search filename…"
                      />
                    </label>
                    <div className="editor-file-list">
                      {editorFiles.length === 0 ? (
                        <p className="empty-state">Load files to browse vault notes.</p>
                      ) : (
                        editorFiles
                          .filter((f) => !editorFileFilter.trim() || f.file_path.toLowerCase().includes(editorFileFilter.trim().toLowerCase()))
                          .map((file) => (
                            <button
                              key={file.file_path}
                              type="button"
                              className={`editor-file-item ${editorSelectedPath === file.file_path ? "active" : ""}`}
                              onClick={() => openEditorFile(file)}
                            >
                              <span className="editor-file-name">{file.file_path.split("/").pop()}</span>
                              <span className="editor-file-path">{file.file_path}</span>
                            </button>
                          ))
                      )}
                    </div>
                  </div>

                  <div className="editor-main">
                    <div className="editor-toolbar">
                      <span className="editor-filename">{editorSelectedPath || "No file selected"}</span>
                      <div className="button-row">
                        <button
                          type="button"
                          className={`button ghost ${!editorPreviewMode ? "active" : ""}`}
                          onClick={() => setEditorPreviewMode(false)}
                        >
                          Edit
                        </button>
                        <button
                          type="button"
                          className={`button ghost ${editorPreviewMode ? "active" : ""}`}
                          onClick={() => setEditorPreviewMode(true)}
                        >
                          Preview
                        </button>
                        <button
                          type="button"
                          className="button primary"
                          onClick={saveEditorFile}
                          disabled={!editorSelectedPath || editorSaving}
                        >
                          {editorSaving ? "Saving…" : "Save"}
                        </button>
                      </div>
                    </div>

                    {editorPreviewMode ? (
                      <div
                        className="editor-preview markdown-body"
                        dangerouslySetInnerHTML={{ __html: DOMPurify.sanitize(marked.parse(editorContent)) }}
                      />
                    ) : (
                      <textarea
                        className="editor-textarea"
                        value={editorContent}
                        onChange={(event) => setEditorContent(event.target.value)}
                        spellCheck={false}
                        placeholder="Select a file to edit…"
                      />
                    )}
                  </div>
                </div>
              </Panel>
            </>
          ) : null}
        </div>

        <div className="secondary-column">
          <Panel title="Activity feed" eyebrow="Recent actions" detail={isPending ? "UI is applying a response update." : "Recent actions from this session."}>
            <Feed items={feedItems} />
          </Panel>

          <Panel title="Quick notes" eyebrow="Working assumptions">
            <ul className="bullet-list">
              <li>This UI stores the API key and default vault UID in local browser storage only.</li>
              <li>Protected endpoints still require a valid Foundation API key.</li>
              <li>The bundled Nginx proxy forwards <code>/api/*</code> to the existing <code>main</code> service.</li>
              <li>Metadata strings are passed straight through to the backend, so valid JSON strings are safest.</li>
            </ul>
          </Panel>
        </div>
      </div>
    </AppShell>
  );
}

export default App;
