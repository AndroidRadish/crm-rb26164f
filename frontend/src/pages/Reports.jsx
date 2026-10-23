import { useEffect, useState } from 'react'
import { useNavigate } from 'react-router-dom'
import * as api from '../api'
import { Page, Sidebar, getNavItems } from '../components/Layout'
import {
  Button, Callout, Card, CardSkeleton, EmptyState, ErrorState, PageHeader, StatCard,
  Table, TableSkeleton, Tag, fmtDateTime,
} from '../components/ui'
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

  const isEmptyPeriod = Boolean(summary)
    && summary.month_new_customers === 0
    && summary.won_count === 0
    && summary.lost_count === 0
    && summary.overdue_count === 0
    && overdue.length === 0

  return (
    <div className="layout">
      <Sidebar navItems={getNavItems(user)} />
      <Page>
        <PageHeader
          title={`报表${isAdmin ? '（全团队）' : '（我的）'}`}
          description="本月新增、成交率与逾期跟进"
          actions={allowed
            ? <Button variant="secondary" onClick={load} disabled={loading}>刷新</Button>
            : undefined}
        />

        {!allowed ? (
          <FeatureNotice />
        ) : loading ? (
          <>
            <CardSkeleton count={4} />
            <TableSkeleton rows={4} />
          </>
        ) : error && !summary ? (
          <ErrorState message={error} onRetry={load} />
        ) : (
          <>
            {error && (
              <Callout tone="danger" actions={<Button size="sm" variant="secondary" onClick={load}>重试</Button>}>
                {error}
              </Callout>
            )}

            {isEmptyPeriod && (
              <Callout tone="info" title="本期暂无数据">
                本月还没有新增客户或成交记录；数据会随客户跟进与阶段推进自动累计。
              </Callout>
            )}

            {summary && (
              <div className="stat-grid">
                <StatCard label="本月新增客户" value={summary.month_new_customers} />
                <StatCard
                  label="成交率（赢单/总推进）"
                  value={`${summary.win_rate.toFixed(1)}%`}
                  tone="accent"
                />
                <StatCard label="赢单 / 输单" value={`${summary.won_count} / ${summary.lost_count}`} />
                <StatCard
                  label="跟进逾期任务"
                  value={summary.overdue_count}
                  tone={summary.overdue_count > 0 ? 'danger' : 'default'}
                />
              </div>
            )}

            <Card title="跟进逾期列表" subtitle="按截止时间筛选出的未完成待办">
              {overdue.length === 0 ? (
                <EmptyState
                  inline
                  title="没有逾期的跟进任务"
                  description="当前所有待办都在截止时间之内，继续保持。"
                  action={<Button size="sm" variant="secondary" onClick={() => navigate('/tasks')}>查看我的待办</Button>}
                />
              ) : (
                <Table columns={['任务', '客户', ...(isAdmin ? ['负责人'] : []), '截止时间']}>
                  {overdue.map((t) => (
                    <tr
                      key={t.id}
                      className="row-overdue is-clickable"
                      onClick={() => navigate(`/customers/${t.customer_id}`)}
                    >
                      <td>{t.title}</td>
                      <td>{t.customer_company || '—'}</td>
                      {isAdmin && <td>{t.assignee_name || '—'}</td>}
                      <td>
                        <div className="u-row u-gap-2">
                          <span>{fmtDateTime(t.due_at)}</span>
                          <Tag tone="danger">已逾期</Tag>
                        </div>
                      </td>
                    </tr>
                  ))}
                </Table>
              )}
            </Card>
          </>
        )}
      </Page>
    </div>
  )
}
