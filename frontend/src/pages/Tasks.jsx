import { useEffect, useState } from 'react'
import * as api from '../api'
import { Page, Sidebar } from '../components/Layout'
import { fmtDateTime, TableSkeleton, EmptyState } from '../components/ui'
import { getErrorMessage } from '../api/client'

const navItems = [
  { to: '/customers', icon: 'customers', label: '客户库', end: false },
  { to: '/kanban', icon: 'kanban', label: '销售看板', end: false },
  { to: '/dashboard', icon: 'dashboard', label: '数据看板', end: false },
  { to: '/tasks', icon: 'tasks', label: '我的待办', end: false },
]

export default function Tasks() {
  const [tasks, setTasks] = useState([])
  const [loading, setLoading] = useState(true)
  const [error, setError] = useState('')

  const load = async () => {
    setLoading(true)
    try {
      const data = await api.listTasks()
      setTasks(data)
      setError('')
    } catch (err) {
      setError(getErrorMessage(err, '加载待办失败'))
    } finally {
      setLoading(false)
    }
  }

  useEffect(() => { load() }, [])

  const updateStatus = async (id, status) => {
    try {
      await api.updateTask(id, { status })
      load()
    } catch (err) {
      setError(getErrorMessage(err, '更新待办失败'))
    }
  }

  const remove = async (id) => {
    try {
      await api.deleteTask(id)
      load()
    } catch (err) {
      setError(getErrorMessage(err, '删除待办失败'))
    }
  }

  const pending = tasks.filter((t) => t.status === 'pending')
  const done = tasks.filter((t) => t.status === 'done')
  const cancelled = tasks.filter((t) => t.status === 'cancelled')

  return (
    <div className="layout">
      <Sidebar navItems={navItems} />
      <Page>
        <h1 className="page-title">我的待办</h1>
        {error && <div className="form-error">{error}</div>}
        {loading ? (
          <TableSkeleton rows={4} />
        ) : tasks.length === 0 ? (
          <EmptyState message="暂无待办任务" />
        ) : (
          <>
            <div className="card" style={{ marginBottom: 24 }}>
              <div className="card-title">待处理（{pending.length}）</div>
              {pending.length === 0 ? (
                <div className="empty-state">没有待处理任务</div>
              ) : (
                pending.map((t) => (
                  <div key={t.id} className="task-item">
                    <div style={{ flex: 1 }}>
                      <div className="task-title">{t.title}</div>
                      <div className="task-due">截止：{fmtDateTime(t.due_at)}</div>
                    </div>
                    <div style={{ display: 'flex', gap: 4 }}>
                      <button className="btn btn-secondary btn-sm" onClick={() => updateStatus(t.id, 'done')}>完成</button>
                      <button className="btn btn-ghost btn-sm" onClick={() => updateStatus(t.id, 'cancelled')}>取消</button>
                      <button className="btn btn-danger btn-sm" onClick={() => remove(t.id)}>删除</button>
                    </div>
                  </div>
                ))
              )}
            </div>

            {done.length > 0 && (
              <div className="card" style={{ marginBottom: 24 }}>
                <div className="card-title">已完成（{done.length}）</div>
                {done.map((t) => (
                  <div key={t.id} className="task-item">
                    <div style={{ flex: 1 }}>
                      <div className="task-title" style={{ textDecoration: 'line-through', opacity: 0.6 }}>{t.title}</div>
                      <div className="task-due">截止：{fmtDateTime(t.due_at)}</div>
                    </div>
                    <span className="badge badge-settled">已完成</span>
                  </div>
                ))}
              </div>
            )}

            {cancelled.length > 0 && (
              <div className="card">
                <div className="card-title">已取消（{cancelled.length}）</div>
                {cancelled.map((t) => (
                  <div key={t.id} className="task-item">
                    <div style={{ flex: 1 }}>
                      <div className="task-title" style={{ opacity: 0.5 }}>{t.title}</div>
                      <div className="task-due">截止：{fmtDateTime(t.due_at)}</div>
                    </div>
                    <span className="badge badge-past">已取消</span>
                  </div>
                ))}
              </div>
            )}
          </>
        )}
      </Page>
    </div>
  )
}
