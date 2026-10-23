import { useEffect, useState } from 'react'
import * as api from '../api'
import { Page, Sidebar, getNavItems } from '../components/Layout'
import {
  Button, Callout, EmptyState, ErrorState, PageHeader, Table, TableSkeleton, fmtDateTime,
} from '../components/ui'
import { getErrorMessage } from '../api/client'
import { useAuth } from '../auth'

export default function Audit() {
  const { user } = useAuth()
  const [logs, setLogs] = useState([])
  const [loading, setLoading] = useState(true)
  const [error, setError] = useState('')

  const load = async () => {
    setLoading(true)
    try {
      const data = await api.listAudit(200)
      setLogs(data)
      setError('')
    } catch (err) {
      setError(getErrorMessage(err, '加载审计日志失败'))
    } finally {
      setLoading(false)
    }
  }

  useEffect(() => { load() }, [])

  return (
    <div className="layout">
      <Sidebar navItems={getNavItems(user)} />
      <Page>
        <PageHeader
          title="审计日志"
          description="最近 200 条操作留痕，含操作者、实体与详情"
          actions={<Button variant="secondary" onClick={load} disabled={loading}>刷新</Button>}
        />

        {error && logs.length === 0 ? (
          <ErrorState message={error} onRetry={load} />
        ) : (
          <>
            {error && (
              <Callout tone="danger" actions={<Button size="sm" variant="secondary" onClick={load}>重试</Button>}>
                {error}
              </Callout>
            )}

            {loading ? (
              <TableSkeleton rows={6} />
            ) : logs.length === 0 ? (
              <EmptyState
                title="暂无审计日志"
                description="成员新增、编辑、停用等操作发生后，会在这里留下记录。"
              />
            ) : (
              <Table columns={['时间', '用户', '操作', '实体', '详情']}>
                {logs.map((l, i) => (
                  <tr key={i}>
                    <td className="cell-muted">{fmtDateTime(l.created_at)}</td>
                    <td>{l.username || '—'}</td>
                    <td>{l.action}</td>
                    <td className="cell-muted">{l.entity_type}</td>
                    <td>
                      <div className="cell-truncate" title={l.detail || ''}>
                        {l.detail || '—'}
                      </div>
                    </td>
                  </tr>
                ))}
              </Table>
            )}
          </>
        )}
      </Page>
    </div>
  )
}
