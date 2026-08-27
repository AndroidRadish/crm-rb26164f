import { useEffect, useState } from 'react'
import * as api from '../api'
import { Page, Sidebar, getNavItems } from '../components/Layout'
import { fmtDateTime, TableSkeleton, EmptyState } from '../components/ui'
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
        <h1 className="page-title">审计日志</h1>
        {error && <div className="form-error">{error}</div>}
        {loading ? (
          <TableSkeleton rows={6} />
        ) : logs.length === 0 ? (
          <EmptyState message="暂无审计日志" />
        ) : (
          <div className="table-wrap">
            <table className="data">
              <thead>
                <tr>
                  <th>时间</th>
                  <th>用户</th>
                  <th>操作</th>
                  <th>实体</th>
                  <th>详情</th>
                </tr>
              </thead>
              <tbody>
                {logs.map((l, i) => (
                  <tr key={i}>
                    <td>{fmtDateTime(l.created_at)}</td>
                    <td>{l.username || '—'}</td>
                    <td>{l.action}</td>
                    <td>{l.entity_type}</td>
                    <td style={{ maxWidth: 300, overflow: 'hidden', textOverflow: 'ellipsis', whiteSpace: 'nowrap' }}>
                      {l.detail || '—'}
                    </td>
                  </tr>
                ))}
              </tbody>
            </table>
          </div>
        )}
      </Page>
    </div>
  )
}
