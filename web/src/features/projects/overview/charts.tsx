// ProjectOverview.jsx#Spark et #Bar : histogramme compact des indicateurs et
// jauge fine des ressources. Une série vide laisse la place vide (aucune
// donnée inventée) ; une jauge sans mesure reste vide.

export function Spark({ data, h = 28, hl }: { data: number[]; h?: number; hl?: ((v: number, i: number) => boolean) | null }) {
  const max = data.length ? Math.max(...data) : 0;
  return (
    <div role="img" aria-label={data.length ? "Tendance" : "Aucune mesure"} style={{ display: "flex", alignItems: "flex-end", gap: 2, height: h }}>
      {data.map((v, i) => (
        <span key={i} style={{ flex: 1, height: Math.max(2, max > 0 ? (v / max) * h : 0), borderRadius: 1, background: hl && hl(v, i) ? "var(--red-500)" : i === data.length - 1 ? "var(--gray-900)" : "var(--viz-3)" }} />
      ))}
    </div>
  );
}

export function Bar({ v, warn = 80 }: { v?: number | null; warn?: number }) {
  const value = v ?? 0;
  return (
    <span style={{ display: "block", height: 4, borderRadius: 99, background: "var(--viz-4)", overflow: "hidden" }}>
      <span style={{ display: "block", height: "100%", width: value + "%", borderRadius: 99, background: value >= warn ? "#e5b454" : "var(--viz-1)" }} />
    </span>
  );
}
