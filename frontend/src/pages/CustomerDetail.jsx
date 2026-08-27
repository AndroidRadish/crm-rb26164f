import { useEffect, useState } from 'react'
import { useParams, useNavigate } from 'react-router-dom'
import * as api from '../api'
import { Page, Sidebar, getNavItems } from '../components/Layout'
import { fmtDateTime, TableSkeleton, EmptyState } from '../components/ui'
import { getErrorMessage } from '../api/client'
import { useAuth } from '../auth'
import { FeatureNotice } from '../components/FeatureGate'

const ACTIVITY_TYPE_LABEL = {
  call: '📞 电话',
  email: '✉️ 邮件',
  meeting: '🤝 会议',
  note: '📝 备注',
  other: '📌 其他',
}

export default function CustomerDetail() {
  const { id } = useParams()
  const navigate = useNavigate()
  const { user, hasFeature } = useAuth()
  const isAdmin = user?.role === 'admin'
  const [customer, setCustomer] = useState(null)
  const [activities, setActivities] = useState([])
  const [tasks, setTasks] = useState([])
  const [stages, setStages] = useState([])
  const [loading, setLoading] = useState(true)
  const [error, setError] = useState('')
  const [activityForm, setActivityForm] = useState({ type: 'call', content: '' })
  const [taskForm, setTaskForm] = useState({ title: '', due_at: '' })
  const [editMode, setEditMode] = useState(false)
  const [editForm, setEditForm] = useState({})
  const [transferMode, setTransferMode] = useState(false)
  const [teamUsers, setTeamUsers] = useState([])
  const [transferTo, setTransferTo] = useState('')
  const [transferring, setTransferring] = useState(false)

  const load = async () => {
    setLoading(true)
    try {
      const [c, acts, ts, sts] = await Promise.all([
        api.getCustomer(id),
        api.listActivities(id),
        api.listCustomerTasks(id),
        api.listStages(),
      ])
      setCustomer(c)
      setActivities(acts)
      setTasks(ts)
      setStages(sts)
      setEditForm({
        company: c.company,
        contact_name: c.contact_name || '',
        phone: c.phone || '',
        email: c.email || '',
        address: c.address || '',
        notes: c.notes || '',
        stage: c.stage || '',
      })
      setError('')
    } catch (err) {
      setError(getErrorMessage(err, '加载客户详情失败'))
    } finally {
      setLoading(false)
    }
  }

  useEffect(() => { load() }, [id])

  const addActivity = async (e) => {
    e.preventDefault()
    if (!activityForm.content.trim()) return
    try {
      await api.createActivity(id, activityForm)
      setActivityForm({ type: 'call', content: '' })
      load()
    } catch (err) {
      setError(getErrorMessage(err, '添加跟进失败'))
    }
  }

  const addTask = async (e) => {
    e.preventDefault()
    if (!taskForm.title.trim() || !taskForm.due_at) return
    try {
      await api.createTask(id, { title: taskForm.title, due_at: new Date(taskForm.due_at).toISOString() })
      setTaskForm({ title: '', due_at: '' })
      load()
    } catch (err) {
      setError(getErrorMessage(err, '创建待办失败'))
    }
  }

  const updateCustomer = async (e) => {
    e.preventDefault()
    try {
      const data = { ...editForm, stage: editForm.stage || null }
      await api.updateCustomer(id, data)
      setEditMode(false)
      load()
    } catch (err) {
      setError(getErrorMessage(err, '更新失败'))
    }
  }

  const updateTaskStatus = async (taskId, status) => {
    try {
      await api.updateTask(taskId, { status })
      load()
    } catch (err) {
      setError(getErrorMessage(err, '更新待办失败'))
    }
  }

  // 换人（admin）：打开面板时拉取成员列表
  const openTransfer = async () => {
    setTransferMode(true)
    setTransferTo('')
    try {
      const users = await api.listUsers()
      setTeamUsers(users.filter((u) => u.id !== customer?.owner_id))
    } catch (err) {
      setError(getErrorMessage(err, '加载成员列表失败'))
    }
  }

  const doTransfer = async () => {
    if (!transferTo) return
    setTransferring(true)
    setError('')
    try {
      await api.transferCustomer(id, transferTo)
      setTransferMode(false)
      load()
    } catch (err) {
      setError(getErrorMessage(err, '换人失败'))
    } finally {
      setTransferring(false)
    }
  }

  // 逾期判定：pending 且截止时间已过（列表页逾期标红，spec §2.1）
  const isOverdue = (t) => t.status === 'pending' && new Date(t.due_at) < new Date()

  const stageName = (sid) => {
    const s = stages.find((s) => s.id === sid)
    return s ? s.name : '—'
  }

  if (loading) return <div className="layout"><Page><TableSkeleton rows={4} /></Page></div>
  if (error) return <div className="layout"><Page><div className="form-error">{error}</div></Page></div>
  if (!customer) return <div className="layout"><Page><EmptyState message="客户不存在" /></Page></div>

  return (
    <div className="layout">
      <Sidebar navItems={getNavItems(user)} />
      <Page>
        <button className="btn btn-ghost" onClick={() => navigate('/customers')} style={{ marginBottom: 16 }}>← 返回客户库</button>

        <div style={{ display: 'flex', justifyContent: 'space-between', alignItems: 'center', flexWrap: 'wrap', gap: 12 }}>
          <h1 className="page-title">{customer.company}</h1>
          <div style={{ display: 'flex', gap: 8 }}>
            {isAdmin && (
              <button className="btn btn-secondary" onClick={() => (transferMode ? setTransferMode(false) : openTransfer())}>
                {transferMode ? '取消换人' : '换人'}
              </button>
            )}
            <button className="btn btn-secondary" onClick={() => setEditMode(!editMode)}>
              {editMode ? '取消编辑' : '编辑'}
            </button>
          </div>
        </div>

        {transferMode && (
          <div className="card" style={{ marginBottom: 24, maxWidth: 600 }}>
            <div className="card-title">负责人变更（换人）</div>
            <p style={{ fontSize: 13, color: 'var(--color-text-secondary)', margin: '4px 0 12px' }}>
              历史跟进时间线完整保留，新负责人可看到全部历史记录；换人会自动在时间线上留一条交接记录。
            </p>
            <div className="form-group">
              <label className="form-label">新负责人 *</label>
              <select value={transferTo} onChange={(e) => setTransferTo(e.target.value)}>
                <option value="">请选择成员</option>
                {teamUsers.map((u) => (
                  <option key={u.id} value={u.id}>{u.display_name || u.username}</option>
                ))}
              </select>
            </div>
            <button className="btn btn-primary" disabled={!transferTo || transferring} onClick={doTransfer}>
              {transferring ? '换人中…' : '确认换人'}
            </button>
          </div>
        )}

        {editMode ? (
          <div className="card" style={{ marginBottom: 24, maxWidth: 600 }}>
            <div className="card-title">编辑客户</div>
            <form onSubmit={updateCustomer}>
              <div className="form-group">
                <label className="form-label">公司名 *</label>
                <input value={editForm.company} onChange={(e) => setEditForm({ ...editForm, company: e.target.value })} />
              </div>
              <div className="form-group">
                <label className="form-label">联系人</label>
                <input value={editForm.contact_name} onChange={(e) => setEditForm({ ...editForm, contact_name: e.target.value })} />
              </div>
              <div className="form-group">
                <label className="form-label">电话</label>
                <input value={editForm.phone} onChange={(e) => setEditForm({ ...editForm, phone: e.target.value })} />
              </div>
              <div className="form-group">
                <label className="form-label">邮箱</label>
                <input value={editForm.email} onChange={(e) => setEditForm({ ...editForm, email: e.target.value })} />
              </div>
              <div className="form-group">
                <label className="form-label">地址</label>
                <input value={editForm.address} onChange={(e) => setEditForm({ ...editForm, address: e.target.value })} />
              </div>
              <div className="form-group">
                <label className="form-label">备注</label>
                <textarea value={editForm.notes} onChange={(e) => setEditForm({ ...editForm, notes: e.target.value })} rows={3} />
              </div>
              <div className="form-group">
                <label className="form-label">销售阶段</label>
                <select value={editForm.stage} onChange={(e) => setEditForm({ ...editForm, stage: e.target.value })}>
                  <option value="">未设置</option>
                  {stages.map((s) => (
                    <option key={s.id} value={s.id}>{s.name}</option>
                  ))}
                </select>
              </div>
              <button type="submit" className="btn btn-primary">保存</button>
            </form>
          </div>
        ) : (
          <div className="card" style={{ marginBottom: 24 }}>
            <div className="customer-info-grid">
              <div><strong>联系人：</strong>{customer.contact_name || '—'}</div>
              <div><strong>电话：</strong>{customer.phone || '—'}</div>
              <div><strong>邮箱：</strong>{customer.email || '—'}</div>
              <div><strong>地址：</strong>{customer.address || '—'}</div>
              <div><strong>阶段：</strong>{stageName(customer.stage)}</div>
              <div><strong>来源：</strong>{customer.source || '—'}</div>
              <div><strong>创建时间：</strong>{fmtDateTime(customer.created_at)}</div>
              <div><strong>更新时间：</strong>{fmtDateTime(customer.updated_at)}</div>
            </div>
            {customer.notes && (
              <div style={{ marginTop: 12 }}>
                <strong>备注：</strong>{customer.notes}
              </div>
            )}
          </div>
        )}

        <div className="grid grid-2">
          {/* 时间轴（功能分级 crm.activities：无授权隐藏内容，后端 403 是底线） */}
          {hasFeature('crm.activities') ? (
          <div className="card">
            <div className="card-title">跟进时间轴</div>
            <form onSubmit={addActivity} style={{ marginBottom: 16 }}>
              <div style={{ display: 'flex', gap: 8, marginBottom: 8 }}>
                <select
                  value={activityForm.type}
                  onChange={(e) => setActivityForm({ ...activityForm, type: e.target.value })}
                  style={{ maxWidth: 120 }}
                >
                  <option value="call">电话</option>
                  <option value="email">邮件</option>
                  <option value="meeting">会议</option>
                  <option value="note">备注</option>
                  <option value="other">其他</option>
                </select>
                <input
                  placeholder="记录跟进内容…"
                  value={activityForm.content}
                  onChange={(e) => setActivityForm({ ...activityForm, content: e.target.value })}
                />
              </div>
              <button type="submit" className="btn btn-primary btn-sm">添加跟进</button>
            </form>
            <div className="timeline">
              {activities.length === 0 ? (
                <div className="empty-state">暂无跟进记录</div>
              ) : (
                activities.map((a) => (
                  <div key={a.id} className="timeline-item">
                    <div className="timeline-dot" />
                    <div className="timeline-content">
                      <div className="timeline-header">
                        <span className="timeline-type">{ACTIVITY_TYPE_LABEL[a.type] || a.type}</span>
                        {a.author_name && <span className="timeline-author">{a.author_name}</span>}
                        <span className="timeline-date">{fmtDateTime(a.occurred_at)}</span>
                      </div>
                      <div className="timeline-text">{a.content}</div>
                    </div>
                  </div>
                ))
              )}
            </div>
          </div>
          ) : (
            <FeatureNotice />
          )}

          {/* 待办 */}
          <div className="card">
            <div className="card-title">待办任务</div>
            <form onSubmit={addTask} style={{ marginBottom: 16 }}>
              <div style={{ display: 'flex', gap: 8, marginBottom: 8 }}>
                <input
                  placeholder="待办标题…"
                  value={taskForm.title}
                  onChange={(e) => setTaskForm({ ...taskForm, title: e.target.value })}
                />
              </div>
              <div style={{ display: 'flex', gap: 8, marginBottom: 8 }}>
                <input
                  type="datetime-local"
                  value={taskForm.due_at}
                  onChange={(e) => setTaskForm({ ...taskForm, due_at: e.target.value })}
                />
              </div>
              <button type="submit" className="btn btn-primary btn-sm">创建待办</button>
            </form>
            <div>
              {tasks.length === 0 ? (
                <div className="empty-state">暂无待办</div>
              ) : (
                tasks.map((t) => (
                  <div key={t.id} className={`task-item${isOverdue(t) ? ' task-overdue' : ''}`}>
                    <div style={{ flex: 1 }}>
                      <div className="task-title">{t.title}</div>
                      <div className="task-due">
                        截止：{fmtDateTime(t.due_at)}
                        {isOverdue(t) && <span className="badge badge-overdue">已逾期</span>}
                      </div>
                    </div>
                    <div style={{ display: 'flex', gap: 4 }}>
                      {t.status === 'pending' && (
                        <button className="btn btn-secondary btn-sm" onClick={() => updateTaskStatus(t.id, 'done')}>完成</button>
                      )}
                      {t.status === 'pending' && (
                        <button className="btn btn-ghost btn-sm" onClick={() => updateTaskStatus(t.id, 'cancelled')}>取消</button>
                      )}
                      {t.status === 'done' && <span className="badge badge-settled">已完成</span>}
                      {t.status === 'cancelled' && <span className="badge badge-past">已取消</span>}
                    </div>
                  </div>
                ))
              )}
            </div>
          </div>
        </div>
      </Page>
    </div>
  )
}
