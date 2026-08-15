import { useEffect, useState } from 'react'
import * as api from '../api'
import { Page, Sidebar } from '../components/Layout'
import { CardSkeleton } from '../components/ui'
import { getErrorMessage } from '../api/client'

const navItems = [
  { to: '/customers', icon: 'customers', label: '客户库', end: false },
  { to: '/kanban', icon: 'kanban', label: '销售看板', end: false },
  { to: '/dashboard', icon: 'dashboard', label: '数据看板', end: false },
  { to: '/tasks', icon: 'tasks', label: '我的待办', end: false },
]

export default function Dashboard() {
  const [stats, setStats] = useState(null)
  const [funnel, setFunnel] = useState([])
  const [loading, setLoading] = useState(true)
  const [error, setError] = useState('')

  const load = async () => {
    setLoading(true)
    try {
      const [s, f] = await Promise.all([api.getDashboard(), api.getFunnel()])
      setStats(s)
      setFunnel(f)
      setError('')
    } catch (err) {
      setError(getErrorMessage(err, '加载数据看板失败'))
    } finally {
      setLoading(false)
    }
  }

  useEffect(() => { load() }, [])

  if (loading) return <div className="layout"><Page><CardSkeleton count={4} /></Page></div>

  const maxCount = Math.max(...funnel.map((f) => f.count), 1)

  return (
    <div className="layout">
      <Sidebar navItems={navItems} />
      <Page>
        <h1 className="page-title">数据看板</h1>
        {error && <div className="form-error">{error}</div>}

        {stats && (
          <div className="grid grid-4" style={{ marginBottom: 24 }}>
            <div className="card stat-card">
              <div className="stat-label">客户总数</div>
              <div className="stat-value">{stats.total_customers}</div>
            </div>
            <div className="card stat-card">
              <div className="stat-label">跟进次数</div>
              <div className="stat-value">{stats.total_activities}</div>
            </div>
            <div className="card stat-card">
              <div className="stat-label">赢单数</div>
              <div className="stat-value">{stats.won_count}</div>
            </div>
            <div className="card stat-card">
              <div className="stat-label">赢单率</div>
              <div className="stat-value">{stats.win_rate.toFixed(1)}%</div>
            </div>
          </div>
        )}

        <div className="card" style={{ marginBottom: 24 }}>
          <div className="card-title">销售漏斗</div>
          {funnel.length === 0 ? (
            <div className="empty-state">暂无数据</div>
          ) : (
            <div className="funnel-chart">
              {funnel.map((f) => (
                <div key={f.stage_id} className="funnel-row">
                  <div className="funnel-label">{f.name}</div>
                  <div className="funnel-bar-wrap">
                    <div
                      className="funnel-bar"
                      style={{ width: `${(f.count / maxCount) * 100}%` }}
                    >
                      {f.count}
                    </div>
                  </div>
                </div>
              ))}
            </div>
          )}
        </div>

        {stats && stats.pending_tasks > 0 && (
          <div className="card" style={{ borderColor: 'var(--color-accent)' }}>
            <div className="card-title">待办提醒</div>
            <div style={{ fontSize: 14 }}>
              你有 <strong>{stats.pending_tasks}</strong> 个待办任务待处理
            </div>
          </div>
        )}
      </Page>
    </div>
  )
}
