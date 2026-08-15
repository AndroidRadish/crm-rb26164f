import { useEffect, useState } from 'react'
import * as api from '../api'
import { Page, Sidebar } from '../components/Layout'
import { TableSkeleton, EmptyState } from '../components/ui'
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

export default function Stages() {
  const [stages, setStages] = useState([])
  const [loading, setLoading] = useState(true)
  const [error, setError] = useState('')
  const [showForm, setShowForm] = useState(false)
  const [form, setForm] = useState({ name: '', order_index: '', is_won: false, is_lost: false })
  const [formError, setFormError] = useState('')

  const load = async () => {
    setLoading(true)
    try {
      const data = await api.listStages()
      setStages(data)
      setError('')
    } catch (err) {
      setError(getErrorMessage(err, '加载阶段失败'))
    } finally {
      setLoading(false)
    }
  }

  useEffect(() => { load() }, [])

  const create = async (e) => {
    e.preventDefault()
    setFormError('')
    if (!form.name.trim()) { setFormError('请填写阶段名称'); return }
    try {
      await api.createStage({
        name: form.name,
        order_index: form.order_index ? parseInt(form.order_index) : undefined,
        is_won: form.is_won,
        is_lost: form.is_lost,
      })
      setForm({ name: '', order_index: '', is_won: false, is_lost: false })
      setShowForm(false)
      load()
    } catch (err) {
      setFormError(getErrorMessage(err, '创建阶段失败'))
    }
  }

  const update = async (id, data) => {
    try {
      await api.updateStage(id, data)
      load()
    } catch (err) {
      setError(getErrorMessage(err, '更新阶段失败'))
    }
  }

  const remove = async (id) => {
    if (!confirm('确定删除该阶段？')) return
    try {
      await api.deleteStage(id)
      load()
    } catch (err) {
      setError(getErrorMessage(err, '删除阶段失败'))
    }
  }

  return (
    <div className="layout">
      <Sidebar navItems={navItems} />
      <Page>
        <div style={{ display: 'flex', justifyContent: 'space-between', alignItems: 'center' }}>
          <h1 className="page-title">销售阶段管理</h1>
          <button className="btn btn-primary" onClick={() => setShowForm(!showForm)}>
            {showForm ? '取消' : '+ 新增阶段'}
          </button>
        </div>

        {showForm && (
          <div className="card" style={{ marginBottom: 24, maxWidth: 480 }}>
            <div className="card-title">新增销售阶段</div>
            {formError && <div className="form-error">{formError}</div>}
            <form onSubmit={create}>
              <div className="form-group">
                <label className="form-label">阶段名称 *</label>
                <input value={form.name} onChange={(e) => setForm({ ...form, name: e.target.value })} />
              </div>
              <div className="form-group">
                <label className="form-label">排序（数字越小越靠前）</label>
                <input type="number" value={form.order_index} onChange={(e) => setForm({ ...form, order_index: e.target.value })} />
              </div>
              <div className="form-group" style={{ display: 'flex', gap: 16 }}>
                <label style={{ display: 'flex', alignItems: 'center', gap: 6 }}>
                  <input type="checkbox" checked={form.is_won} onChange={(e) => setForm({ ...form, is_won: e.target.checked })} style={{ width: 'auto' }} />
                  赢单
                </label>
                <label style={{ display: 'flex', alignItems: 'center', gap: 6 }}>
                  <input type="checkbox" checked={form.is_lost} onChange={(e) => setForm({ ...form, is_lost: e.target.checked })} style={{ width: 'auto' }} />
                  输单
                </label>
              </div>
              <button type="submit" className="btn btn-primary">创建</button>
            </form>
          </div>
        )}

        {error && <div className="form-error">{error}</div>}
        {loading ? (
          <TableSkeleton rows={4} />
        ) : stages.length === 0 ? (
          <EmptyState message="暂无销售阶段" />
        ) : (
          <div className="table-wrap">
            <table className="data">
              <thead>
                <tr>
                  <th>排序</th>
                  <th>名称</th>
                  <th>类型</th>
                  <th>状态</th>
                  <th>操作</th>
                </tr>
              </thead>
              <tbody>
                {stages.map((s) => (
                  <tr key={s.id}>
                    <td>{s.order_index}</td>
                    <td>{s.name}</td>
                    <td>
                      {s.is_won ? <span className="badge badge-settled">赢单</span> :
                       s.is_lost ? <span className="badge badge-past">输单</span> :
                       <span className="badge badge-lead">进行中</span>}
                    </td>
                    <td>{s.is_active ? '启用' : '停用'}</td>
                    <td>
                      <button
                        className="btn btn-ghost btn-sm"
                        onClick={() => update(s.id, { is_active: !s.is_active })}
                      >
                        {s.is_active ? '停用' : '启用'}
                      </button>
                      <button className="btn btn-danger btn-sm" onClick={() => remove(s.id)}>删除</button>
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
