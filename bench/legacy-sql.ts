/** Atom ranking SQL as of commit d211af5 (ORDER BY GREATEST(...) over every row), kept as the benchmark baseline. */
export const LEGACY_ATOM_SEARCH_SQL = `SELECT *
FROM (
  SELECT a.id,a.content,a.metadata,a.updated_at,
    CASE WHEN lower(a.content)=lower($1) THEN 1 ELSE 0 END AS exact_score,
    GREATEST(
      similarity(a.content,$1),
      ts_rank(to_tsvector('simple',a.content || ' ' || array_to_string(a.tags,' ')),plainto_tsquery('simple',$1))
    ) AS lexical_score,
    similarity(a.content,$8) AS symbol_score,
    CASE WHEN a.tags && $2::text[] THEN 1 ELSE 0 END AS tag_score,
    GREATEST(
      CASE WHEN a.metadata ?| $2::text[] THEN 1 ELSE 0 END,
      similarity(a.metadata::text,$1),
      ts_rank(jsonb_to_tsvector('simple',a.metadata,'["string","key","numeric"]'::jsonb),plainto_tsquery('simple',$1))
    ) AS metadata_score,
    CASE WHEN
      $3::halfvec IS NULL OR
      a.vector IS NULL OR
      a.vector_provider IS DISTINCT FROM $5 OR
      a.vector_model IS DISTINCT FROM $6 OR
      a.vector_dimensions IS DISTINCT FROM $7
    THEN 0 ELSE 1 - (a.vector <=> $3::halfvec) END AS semantic_score,
    COALESCE(
      jsonb_agg(jsonb_build_object('id',f.id,'content',f.content)) FILTER (WHERE f.id IS NOT NULL),
      '[]'::jsonb
    ) AS docked_files
  FROM atoms a
  LEFT JOIN atom_file_docks d ON d.atom_id=a.id
  LEFT JOIN files f ON f.id=d.file_id
  WHERE COALESCE(a.metadata->>'active','true') <> 'false'
    AND NOT EXISTS (
      SELECT 1
      FROM jsonb_each_text($9::jsonb) AS filter(key,value)
      WHERE lower(coalesce(a.metadata ->> filter.key,'')) <> lower(filter.value)
    )
  GROUP BY a.id
) AS ranked
ORDER BY GREATEST(exact_score,lexical_score,symbol_score,tag_score,metadata_score,semantic_score) DESC
LIMIT $4`;
