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
  const [customers, setCustomers] = useState([])
  const [showForm, setShowForm] = useState(false)
  const [form, setForm] = useState({ title: '', due_at: '', customer_id: '' })
  const [submitting, setSubmitting] = useState(false)

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

  // 打开新建表单时加载客户下拉列表（选择现有客户）
  const openForm = async () => {
    setShowForm(true)
    setError('')
    try {
      const data = await api.listCustomers()
      setCustomers(data)
    } catch (err) {
      setError(getErrorMessage(err, '加载客户列表失败'))
    }
  }

  const cancelForm = () => {
    setShowForm(false)
    setForm({ title: '', due_at: '', customer_id: '' })
  }

  const submitCreate = async () => {
    if (!form.title.trim()) { setError('请填写待办标题'); return }
    if (!form.due_at) { setError('请选择截止时间'); return }
    if (!form.customer_id) { setError('请选择关联客户'); return }
    setSubmitting(true)
    setError('')
    try {
      // datetime-local 值视为本地时间，转 RFC3339 传给后端
      const dueAt = new Date(form.due_at).toISOString()
      await api.createTask(form.customer_id, { title: form.title.trim(), due_at: dueAt })
      cancelForm()
      load()
    } catch (err) {
      setError(getErrorMessage(err, '新建待办失败'))
    } finally {
      setSubmitting(false)
    }
  }

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
        <div style={{ display: 'flex', justifyContent: 'space-between', alignItems: 'center', marginBottom: 16 }}>
          <h1 className="page-title" style={{ marginBottom: 0 }}>我的待办</h1>
          {!showForm && (
            <button className="btn btn-primary" onClick={openForm}>新建待办</button>
          )}
        </div>
        {error && <div className="form-error">{error}</div>}

        {showForm && (
          <div className="card" style={{ marginBottom: 24 }}>
            <div className="card-title">新建待办</div>
            <div className="form-group">
              <label className="form-label">标题 *</label>
              <input
                type="text"
                value={form.title}
                onChange={(e) => setForm({ ...form, title: e.target.value })}
                placeholder="例如：回访客户"
              />
            </div>
            <div className="form-group">
              <label className="form-label">截止时间 *</label>
              <input
                type="datetime-local"
                value={form.due_at}
                onChange={(e) => setForm({ ...form, due_at: e.target.value })}
              />
            </div>
            <div className="form-group">
              <label className="form-label">关联客户 *</label>
              <select
                value={form.customer_id}
                onChange={(e) => setForm({ ...form, customer_id: e.target.value })}
              >
                <option value="">请选择客户</option>
                {customers.map((c) => (
                  <option key={c.id} value={c.id}>
                    {c.company}{c.contact_name ? `（${c.contact_name}）` : ''}
                  </option>
                ))}
              </select>
            </div>
            <div style={{ display: 'flex', gap: 8 }}>
              <button className="btn btn-primary" onClick={submitCreate} disabled={submitting}>
                {submitting ? '创建中…' : '创建'}
              </button>
              <button className="btn btn-secondary" onClick={cancelForm} disabled={submitting}>
                取消
              </button>
            </div>
          </div>
        )}

        {loading ? (
          <TableSkeleton rows={4} />
        ) : tasks.length === 0 ? (
          <EmptyState message="暂无待办任务，点击右上角「新建待办」创建" />
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
                    <div style={{ display: 'flex', gap: 4, alignItems: 'center' }}>
                      <span className="badge badge-settled">已完成</span>
                      <button className="btn btn-secondary btn-sm" onClick={() => updateStatus(t.id, 'pending')}>恢复</button>
                      <button className="btn btn-danger btn-sm" onClick={() => remove(t.id)}>删除</button>
                    </div>
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
                    <div style={{ display: 'flex', gap: 4, alignItems: 'center' }}>
                      <span className="badge badge-past">已取消</span>
                      <button className="btn btn-danger btn-sm" onClick={() => remove(t.id)}>删除</button>
                    </div>
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
