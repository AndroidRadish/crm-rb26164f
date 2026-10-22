import { useEffect, useState } from 'react'
import * as api from '../api'
import { Page, Sidebar, getNavItems } from '../components/Layout'
import { fmtDateTime, TableSkeleton, EmptyState } from '../components/ui'
import { getErrorMessage } from '../api/client'
import { useAuth } from '../auth'

export default function Users() {
  const { user } = useAuth()
  const [users, setUsers] = useState([])
  const [loading, setLoading] = useState(true)
  const [error, setError] = useState('')
  const [showForm, setShowForm] = useState(false)
  const [form, setForm] = useState({ username: '', password: '', role: 'member', display_name: '' })
  const [formError, setFormError] = useState('')
  // 成员生命周期（停用/启用/重置密码）
  const [actionError, setActionError] = useState('')
  const [busyId, setBusyId] = useState(null)
  const [resetTarget, setResetTarget] = useState(null)
  const [resetPassword, setResetPassword] = useState('')
  const [resetError, setResetError] = useState('')

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

  // 停用前二次确认（停用即断：该成员立即无法登录，已登录会话下一次请求 401）
  const toggleActive = async (u) => {
    const nextActive = !u.is_active
    if (!nextActive && !window.confirm(`确定停用成员「${u.username}」？停用后该成员立即无法登录，已登录会话也会失效。`)) {
      return
    }
    setActionError('')
    setBusyId(u.id)
    try {
      if (nextActive) {
        await api.activateUser(u.id)
      } else {
        await api.deactivateUser(u.id)
      }
      await load()
    } catch (err) {
      // 后端防线（不能停用自己 / 最后一个管理员）由这里展示给操作者
      setActionError(getErrorMessage(err, nextActive ? '启用成员失败' : '停用成员失败'))
    } finally {
      setBusyId(null)
    }
  }

  const openReset = (u) => {
    setActionError('')
    setResetTarget(u)
    setResetPassword('')
    setResetError('')
  }

  const submitReset = async (e) => {
    e.preventDefault()
    setResetError('')
    if (resetPassword.length < 6) {
      setResetError('密码至少 6 位')
      return
    }
    try {
      await api.resetUserPassword(resetTarget.id, resetPassword)
      setResetTarget(null)
      setResetPassword('')
      load()
    } catch (err) {
      setResetError(getErrorMessage(err, '重置密码失败'))
    }
  }

  return (
    <div className="layout">
      <Sidebar navItems={getNavItems(user)} />
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

        {resetTarget && (
          <div className="card" style={{ marginBottom: 24, maxWidth: 480 }}>
            <div className="card-title">重置密码 — {resetTarget.username}</div>
            <div style={{ color: 'var(--color-text-secondary)', fontSize: 13, marginBottom: 12 }}>
              管理员直接设置新密码，无需旧密码；重置后请把新密码线下告知成员。
            </div>
            {resetError && <div className="form-error">{resetError}</div>}
            <form onSubmit={submitReset}>
              <div className="form-group">
                <label className="form-label">新密码 *（至少 6 位）</label>
                <input type="password" value={resetPassword} onChange={(e) => setResetPassword(e.target.value)} />
              </div>
              <div style={{ display: 'flex', gap: 8 }}>
                <button type="submit" className="btn btn-primary">确认重置</button>
                <button type="button" className="btn btn-secondary" onClick={() => setResetTarget(null)}>取消</button>
              </div>
            </form>
          </div>
        )}

        {error && (
          <div className="form-error" style={{ display: 'flex', gap: 8, alignItems: 'center' }}>
            <span>{error}</span>
            <button type="button" className="btn btn-sm btn-secondary" onClick={load}>重试</button>
          </div>
        )}
        {actionError && <div className="form-error">{actionError}</div>}
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
                  <th>状态</th>
                  <th>创建时间</th>
                  <th>操作</th>
                </tr>
              </thead>
              <tbody>
                {users.map((u) => (
                  <tr key={u.id}>
                    <td>
                      {u.username}
                      {user && u.id === user.id && (
                        <span className="badge badge-lead" style={{ marginLeft: 8 }}>我</span>
                      )}
                    </td>
                    <td>{u.display_name || '—'}</td>
                    <td>
                      <span className={`badge ${u.role === 'admin' ? 'badge-settled' : 'badge-lead'}`}>
                        {u.role === 'admin' ? '管理员' : '成员'}
                      </span>
                    </td>
                    <td>
                      <span className={`badge ${u.is_active ? 'badge-active' : 'badge-past'}`}>
                        {u.is_active ? '启用中' : '已停用'}
                      </span>
                    </td>
                    <td>{fmtDateTime(u.created_at)}</td>
                    <td>
                      <div style={{ display: 'flex', gap: 8 }}>
                        <button
                          type="button"
                          className={`btn btn-sm ${u.is_active ? 'btn-danger' : 'btn-secondary'}`}
                          disabled={busyId === u.id}
                          onClick={() => toggleActive(u)}
                        >
                          {busyId === u.id ? '处理中…' : u.is_active ? '停用' : '启用'}
                        </button>
                        <button
                          type="button"
                          className="btn btn-sm btn-secondary"
                          onClick={() => openReset(u)}
                        >
                          重置密码
                        </button>
                      </div>
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
