import { useEffect, useState } from 'react'
import { useNavigate } from 'react-router-dom'
import * as api from '../api'
import { Page, Sidebar } from '../components/Layout'
import { fmtDate, TableSkeleton, EmptyState } from '../components/ui'
import { getErrorMessage } from '../api/client'
import { useAuth } from '../auth'

const navItems = [
  { to: '/customers', icon: 'customers', label: '客户库', end: false },
  { to: '/kanban', icon: 'kanban', label: '销售看板', end: false },
  { to: '/dashboard', icon: 'dashboard', label: '数据看板', end: false },
  { to: '/tasks', icon: 'tasks', label: '我的待办', end: false },
]

export default function Customers() {
  const { user, hasFeature } = useAuth()
  const canPool = hasFeature('pool.claim')
  const navigate = useNavigate()
  const [customers, setCustomers] = useState([])
  const [stages, setStages] = useState([])
  const [loading, setLoading] = useState(true)
  const [error, setError] = useState('')
  const [search, setSearch] = useState('')
  const [stageFilter, setStageFilter] = useState('')
  const [ownerFilter, setOwnerFilter] = useState('')
  const [showForm, setShowForm] = useState(false)
  const [form, setForm] = useState({ company: '', contact_name: '', phone: '', email: '', address: '', notes: '', stage: '' })
  const [formError, setFormError] = useState('')
  const [dupWarning, setDupWarning] = useState(null)

  const load = async () => {
    setLoading(true)
    try {
      const params = {}
      if (search) params.search = search
      if (stageFilter) params.stage = stageFilter
      if (ownerFilter) params.owner = ownerFilter
      const data = await api.listCustomers(params)
      setCustomers(data)
      setError('')
    } catch (err) {
      setError(getErrorMessage(err, '加载客户失败'))
    } finally {
      setLoading(false)
    }
  }

  const loadStages = async () => {
    try {
      const data = await api.listStages()
      setStages(data)
    } catch { /* ignore */ }
  }

  useEffect(() => { load(); loadStages() }, [])

  const create = async (e) => {
    e.preventDefault()
    setFormError('')
    setDupWarning(null)
    if (!form.company.trim()) { setFormError('请填写公司名'); return }

    // 防撞单检测
    try {
      const dup = await api.checkDuplicate({ company: form.company, phone: form.phone })
      if (dup.is_duplicate) {
        setDupWarning(dup.duplicates)
        return
      }
    } catch { /* continue */ }

    try {
      const data = { ...form, stage: form.stage || null }
      await api.createCustomer(data)
      setForm({ company: '', contact_name: '', phone: '', email: '', address: '', notes: '', stage: '' })
      setShowForm(false)
      load()
    } catch (err) {
      setFormError(getErrorMessage(err, '创建失败'))
    }
  }

  const claim = async (id) => {
    try {
      await api.claimCustomer(id)
      load()
    } catch (err) {
      setError(getErrorMessage(err, '认领失败'))
    }
  }

  const release = async (id) => {
    try {
      await api.releaseCustomer(id)
      load()
    } catch (err) {
      setError(getErrorMessage(err, '释放失败'))
    }
  }

  const stageName = (id) => {
    const s = stages.find((s) => s.id === id)
    return s ? s.name : '—'
  }

  return (
    <div className="layout">
      <Sidebar navItems={navItems} />
      <Page>
        <div style={{ display: 'flex', justifyContent: 'space-between', alignItems: 'center', flexWrap: 'wrap', gap: 12 }}>
          <h1 className="page-title">客户库</h1>
          <button className="btn btn-primary" onClick={() => setShowForm(!showForm)}>
            {showForm ? '取消' : '+ 新增客户'}
          </button>
        </div>

        {/* 搜索/筛选 */}
        <div className="filter-bar">
          <input
            placeholder="搜索公司/联系人/电话…"
            value={search}
            onChange={(e) => setSearch(e.target.value)}
            onKeyDown={(e) => e.key === 'Enter' && load()}
            style={{ maxWidth: 240 }}
          />
          <select value={stageFilter} onChange={(e) => { setStageFilter(e.target.value); load() }} style={{ maxWidth: 140 }}>
            <option value="">全部阶段</option>
            {stages.map((s) => (
              <option key={s.id} value={s.id}>{s.name}</option>
            ))}
          </select>
          <select value={ownerFilter} onChange={(e) => { setOwnerFilter(e.target.value); load() }} style={{ maxWidth: 140 }}>
            <option value="">全部归属</option>
            <option value="me">我的客户</option>
            <option value="pool">公海池</option>
          </select>
          <button className="btn btn-secondary" onClick={load}>查询</button>
        </div>

        {showForm && (
          <div className="card" style={{ marginBottom: 24, maxWidth: 600 }}>
            <div className="card-title">新增客户</div>
            {formError && <div className="form-error">{formError}</div>}
            {dupWarning && dupWarning.length > 0 && (
              <div className="dup-warning">
                <strong>⚠ 检测到疑似重复客户：</strong>
                {dupWarning.map((d) => (
                  <div key={d.id} style={{ marginTop: 4 }}>
                    {d.company} · {d.contact_name || '—'} · {d.phone || '—'}
                  </div>
                ))}
                <div style={{ marginTop: 8 }}>
                  <button className="btn btn-secondary" onClick={() => setDupWarning(null)}>仍要创建</button>
                </div>
              </div>
            )}
            <form onSubmit={create}>
              <div className="form-group">
                <label className="form-label">公司名 *</label>
                <input value={form.company} onChange={(e) => setForm({ ...form, company: e.target.value })} />
              </div>
              <div className="form-group">
                <label className="form-label">联系人</label>
                <input value={form.contact_name} onChange={(e) => setForm({ ...form, contact_name: e.target.value })} />
              </div>
              <div className="form-group">
                <label className="form-label">电话</label>
                <input value={form.phone} onChange={(e) => setForm({ ...form, phone: e.target.value })} />
              </div>
              <div className="form-group">
                <label className="form-label">邮箱</label>
                <input value={form.email} onChange={(e) => setForm({ ...form, email: e.target.value })} />
              </div>
              <div className="form-group">
                <label className="form-label">地址</label>
                <input value={form.address} onChange={(e) => setForm({ ...form, address: e.target.value })} />
              </div>
              <div className="form-group">
                <label className="form-label">备注</label>
                <textarea value={form.notes} onChange={(e) => setForm({ ...form, notes: e.target.value })} rows={3} />
              </div>
              <div className="form-group">
                <label className="form-label">销售阶段</label>
                <select value={form.stage} onChange={(e) => setForm({ ...form, stage: e.target.value })}>
                  <option value="">未设置</option>
                  {stages.map((s) => (
                    <option key={s.id} value={s.id}>{s.name}</option>
                  ))}
                </select>
              </div>
              <button type="submit" className="btn btn-primary">创建</button>
            </form>
          </div>
        )}

        {error && <div className="form-error">{error}</div>}
        {loading ? (
          <TableSkeleton rows={6} />
        ) : customers.length === 0 ? (
          <EmptyState message="暂无客户，点击右上角新增" />
        ) : (
          <div className="table-wrap">
            <table className="data">
              <thead>
                <tr>
                  <th>公司名</th>
                  <th>联系人</th>
                  <th>电话</th>
                  <th>阶段</th>
                  <th>归属</th>
                  <th>更新时间</th>
                  <th>操作</th>
                </tr>
              </thead>
              <tbody>
                {customers.map((c) => (
                  <tr key={c.id} onClick={() => navigate(`/customers/${c.id}`)} style={{ cursor: 'pointer' }}>
                    <td>{c.company}</td>
                    <td>{c.contact_name || '—'}</td>
                    <td>{c.phone || '—'}</td>
                    <td><span className="badge badge-lead">{stageName(c.stage)}</span></td>
                    <td>{c.owner_id ? (c.owner_id === user?.id ? '我' : '他人') : '公海'}</td>
                    <td>{fmtDate(c.updated_at)}</td>
                    <td onClick={(e) => e.stopPropagation()}>
                      {!c.owner_id && canPool && (
                        <button className="btn btn-secondary btn-sm" onClick={() => claim(c.id)}>认领</button>
                      )}
                      {c.owner_id === user?.id && canPool && (
                        <button className="btn btn-ghost btn-sm" onClick={() => release(c.id)}>释放</button>
                      )}
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
