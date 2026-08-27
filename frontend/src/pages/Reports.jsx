import { useEffect, useState } from 'react'
import { useNavigate } from 'react-router-dom'
import * as api from '../api'
import { Page, Sidebar, getNavItems } from '../components/Layout'
import { fmtDateTime, TableSkeleton, EmptyState } from '../components/ui'
import { getErrorMessage } from '../api/client'
import { useAuth } from '../auth'
import { FeatureNotice } from '../components/FeatureGate'

// 报表页（RB-266FB3 §2.4）：本月新增客户 / 成交率 / 跟进逾期列表
// 口径：admin 全团队，member 仅自己（后端 scope 过滤保证，前端只展示）
export default function Reports() {
  const navigate = useNavigate()
  const { user, hasFeature } = useAuth()
  const isAdmin = user?.role === 'admin'
  const allowed = hasFeature('crm.reports')
  const [summary, setSummary] = useState(null)
  const [overdue, setOverdue] = useState([])
  const [loading, setLoading] = useState(true)
  const [error, setError] = useState('')

  const load = async () => {
    setLoading(true)
    try {
      const [s, o] = await Promise.all([api.getReportsSummary(), api.listOverdueTasks()])
      setSummary(s)
      setOverdue(o)
      setError('')
    } catch (err) {
      setError(getErrorMessage(err, '加载报表失败'))
    } finally {
      setLoading(false)
    }
  }

  useEffect(() => {
    if (allowed) load()
    else setLoading(false)
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [allowed])

  return (
    <div className="layout">
      <Sidebar navItems={getNavItems(user)} />
      <Page>
        <h1 className="page-title">报表{isAdmin ? '（全团队）' : '（我的）'}</h1>
        {!allowed ? (
          <FeatureNotice />
        ) : loading ? (
          <TableSkeleton rows={4} />
        ) : (
          <>
            {error && <div className="form-error">{error}</div>}
            {summary && (
              <div className="grid grid-4" style={{ marginBottom: 24 }}>
                <div className="card stat-card">
                  <div className="stat-label">本月新增客户</div>
                  <div className="stat-value">{summary.month_new_customers}</div>
                </div>
                <div className="card stat-card">
                  <div className="stat-label">成交率（赢单/总推进）</div>
                  <div className="stat-value">{summary.win_rate.toFixed(1)}%</div>
                </div>
                <div className="card stat-card">
                  <div className="stat-label">赢单 / 输单</div>
                  <div className="stat-value">{summary.won_count} / {summary.lost_count}</div>
                </div>
                <div className="card stat-card">
                  <div className="stat-label">跟进逾期任务</div>
                  <div className="stat-value" style={summary.overdue_count > 0 ? { color: 'var(--status-overdue, #dc2626)' } : undefined}>
                    {summary.overdue_count}
                  </div>
                </div>
              </div>
            )}

            <div className="card">
              <div className="card-title">跟进逾期列表</div>
              {overdue.length === 0 ? (
                <EmptyState message="没有逾期的跟进任务" />
              ) : (
                <div className="table-wrap">
                  <table className="data">
                    <thead>
                      <tr>
                        <th>任务</th>
                        <th>客户</th>
                        {isAdmin && <th>负责人</th>}
                        <th>截止时间</th>
                      </tr>
                    </thead>
                    <tbody>
                      {overdue.map((t) => (
                        <tr
                          key={t.id}
                          className="row-overdue"
                          style={{ cursor: 'pointer' }}
                          onClick={() => navigate(`/customers/${t.customer_id}`)}
                        >
                          <td>{t.title}</td>
                          <td>{t.customer_company || '—'}</td>
                          {isAdmin && <td>{t.assignee_name || '—'}</td>}
                          <td>
                            {fmtDateTime(t.due_at)} <span className="badge badge-overdue">已逾期</span>
                          </td>
                        </tr>
                      ))}
                    </tbody>
                  </table>
                </div>
              )}
            </div>
          </>
        )}
      </Page>
    </div>
  )
}
