export function fmtDate(iso) {
  if (!iso) return '—'
  return new Date(iso).toLocaleDateString('zh-CN', { year: 'numeric', month: '2-digit', day: '2-digit' })
}

export function fmtDateTime(iso) {
  if (!iso) return '—'
  return new Date(iso).toLocaleString('zh-CN', {
    month: '2-digit', day: '2-digit', hour: '2-digit', minute: '2-digit',
  })
}

export function TableSkeleton({ rows = 6, cols = 5 }) {
  return (
    <div className="table-wrap">
      <div style={{ padding: '16px 16px 8px' }}>
        {Array.from({ length: rows }).map((_, i) => (
          <div key={i} className="skeleton skeleton-row" style={{ width: '100%' }} />
        ))}
      </div>
    </div>
  )
}

export function CardSkeleton({ count = 4 }) {
  return (
    <div className="grid grid-2" style={{ marginBottom: 24 }}>
      {Array.from({ length: count }).map((_, i) => (
        <div key={i} className="card">
          <div className="skeleton skeleton-row" style={{ width: '40%' }} />
          <div className="skeleton" style={{ height: 28, width: '60%', marginBottom: 12 }} />
          <div className="skeleton skeleton-row" style={{ width: '80%' }} />
        </div>
      ))}
    </div>
  )
}

export function EmptyState({ message }) {
  return <div className="empty-state">{message}</div>
}
