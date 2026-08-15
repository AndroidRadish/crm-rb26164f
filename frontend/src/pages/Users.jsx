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
  { to: '/stages', icon: 'stages', label: '销售阶段', end: false },
  { to: '/import', icon: 'import', label: '数据导入', end: false },
  { to: '/users', icon: 'users', label: '成员管理', end: false },
  { to: '/audit', icon: 'audit', label: '审计日志', end: false },
]

export default function Users() {
  const [users, setUsers] = useState([])
  const [loading, setLoading] = useState(true)
  const [error, setError] = useState('')
  const [showForm, setShowForm] = useState(false)
  const [form, setForm] = useState({ username: '', password: '', role: 'member', display_name: '' })
  const [formError, setFormError] = useState('')

  const load = async () => {
    setLoading(true)
    try {
      const data = await api.listUsers()
      setUsers(data)
      setError('')
    } catch (err) {
      setError(getErrorMessage(err, '加载成员失败'))
    } finally {
      setLoading(false)
    }
  }

  useEffect(() => { load() }, [])

  const create = async (e) => {
    e.preventDefault()
    setFormError('')
    if (!form.username.trim() || !form.password.trim()) {
      setFormError('请填写用户名和密码')
      return
    }
    if (form.password.length < 6) {
      setFormError('密码至少 6 位')
      return
    }
    try {
      await api.createUser(form.username, form.password, form.role, form.display_name)
      setForm({ username: '', password: '', role: 'member', display_name: '' })
      setShowForm(false)
      load()
    } catch (err) {
      setFormError(getErrorMessage(err, '创建成员失败'))
    }
  }

  return (
    <div className="layout">
      <Sidebar navItems={navItems} />
      <Page>
        <div style={{ display: 'flex', justifyContent: 'space-between', alignItems: 'center' }}>
          <h1 className="page-title">成员管理</h1>
          <button className="btn btn-primary" onClick={() => setShowForm(!showForm)}>
            {showForm ? '取消' : '+ 新增成员'}
          </button>
        </div>

        {showForm && (
          <div className="card" style={{ marginBottom: 24, maxWidth: 480 }}>
            <div className="card-title">新增成员</div>
            {formError && <div className="form-error">{formError}</div>}
            <form onSubmit={create}>
              <div className="form-group">
                <label className="form-label">用户名 *</label>
                <input value={form.username} onChange={(e) => setForm({ ...form, username: e.target.value })} />
              </div>
              <div className="form-group">
                <label className="form-label">密码 *（至少 6 位）</label>
                <input type="password" value={form.password} onChange={(e) => setForm({ ...form, password: e.target.value })} />
              </div>
              <div className="form-group">
                <label className="form-label">显示名称</label>
                <input value={form.display_name} onChange={(e) => setForm({ ...form, display_name: e.target.value })} />
              </div>
              <div className="form-group">
                <label className="form-label">角色</label>
                <select value={form.role} onChange={(e) => setForm({ ...form, role: e.target.value })}>
                  <option value="member">普通成员</option>
                  <option value="admin">管理员</option>
                </select>
              </div>
              <button type="submit" className="btn btn-primary">创建</button>
            </form>
          </div>
        )}

        {error && <div className="form-error">{error}</div>}
        {loading ? (
          <TableSkeleton rows={4} />
        ) : users.length === 0 ? (
          <EmptyState message="暂无成员" />
        ) : (
          <div className="table-wrap">
            <table className="data">
              <thead>
                <tr>
                  <th>用户名</th>
                  <th>显示名称</th>
                  <th>角色</th>
                  <th>创建时间</th>
                </tr>
              </thead>
              <tbody>
                {users.map((u) => (
                  <tr key={u.id}>
                    <td>{u.username}</td>
                    <td>{u.display_name || '—'}</td>
                    <td>
                      <span className={`badge ${u.role === 'admin' ? 'badge-settled' : 'badge-lead'}`}>
                        {u.role === 'admin' ? '管理员' : '成员'}
                      </span>
                    </td>
                    <td>{fmtDateTime(u.created_at)}</td>
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
