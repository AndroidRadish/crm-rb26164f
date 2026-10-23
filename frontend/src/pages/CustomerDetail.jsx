import { useEffect, useState } from 'react'
import { useParams, useNavigate } from 'react-router-dom'
import * as api from '../api'
import { Page, Sidebar, getNavItems } from '../components/Layout'
import {
  Button, Callout, Card, CardSkeleton, EmptyState, ErrorState, Field, Input, Modal,
  PageHeader, Select, TableSkeleton, Tag, Textarea, fmtDateTime,
} from '../components/ui'
import { getErrorMessage } from '../api/client'
import { useAuth } from '../auth'
import { FeatureNotice } from '../components/FeatureGate'

const ACTIVITY_META = {
  call: { label: '电话', tone: 'info' },
  email: { label: '邮件', tone: 'neutral' },
  meeting: { label: '会议', tone: 'accent' },
  note: { label: '备注', tone: 'warning' },
  other: { label: '其他', tone: 'muted' },
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
  const [loadError, setLoadError] = useState('')
  const [actionError, setActionError] = useState('')
  const [activityForm, setActivityForm] = useState({ type: 'call', content: '' })
  const [taskForm, setTaskForm] = useState({ title: '', due_at: '' })
  const [editMode, setEditMode] = useState(false)
  const [editForm, setEditForm] = useState({})
  const [editError, setEditError] = useState('')
  const [savingCustomer, setSavingCustomer] = useState(false)
  const [transferMode, setTransferMode] = useState(false)
  const [teamUsers, setTeamUsers] = useState([])
  const [transferTo, setTransferTo] = useState('')
  const [transferring, setTransferring] = useState(false)
  const [transferError, setTransferError] = useState('')
  const [addingActivity, setAddingActivity] = useState(false)
  const [addingTask, setAddingTask] = useState(false)
  const [updatingTaskId, setUpdatingTaskId] = useState(null)

  const [actsDenied, setActsDenied] = useState(false)
  const [tasksDenied, setTasksDenied] = useState(false)
  const [actsError, setActsError] = useState('')
  const [tasksError, setTasksError] = useState('')

  const load = async () => {
    setLoading(true)
    setActionError('')
    // 客户主档与阶段为关键数据：失败才整页报错
    try {
      const [c, sts] = await Promise.all([
        api.getCustomer(id),
        api.listStages(),
      ])
      setCustomer(c)
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
      setLoadError('')
    } catch (err) {
      setLoadError(getErrorMessage(err, '加载客户详情失败'))
      setLoading(false)
      return
    }

    // 时间线/待办独立加载：owner 关闭 crm.activities / crm.tasks 后后端 403，
    // 只隐藏对应卡片（FeatureNotice），不拖垮整页（spec §2.6「前端隐藏入口」）
    if (hasFeature('crm.activities')) {
      try {
        setActivities(await api.listActivities(id))
        setActsDenied(false)
        setActsError('')
      } catch (err) {
        if (err?.response?.status === 403) {
          setActsDenied(true)
        } else {
          setActsError(getErrorMessage(err, '加载跟进记录失败'))
        }
      }
    } else {
      setActivities([])
    }
    if (hasFeature('crm.tasks')) {
      try {
        setTasks(await api.listCustomerTasks(id))
        setTasksDenied(false)
        setTasksError('')
      } catch (err) {
        if (err?.response?.status === 403) {
          setTasksDenied(true)
        } else {
          setTasksError(getErrorMessage(err, '加载待办失败'))
        }
      }
    } else {
      setTasks([])
    }
    setLoading(false)
  }

  useEffect(() => { load() }, [id])

  const addActivity = async (e) => {
    e.preventDefault()
    if (!activityForm.content.trim()) return
    setAddingActivity(true)
    setActionError('')
    try {
      await api.createActivity(id, activityForm)
      setActivityForm({ type: 'call', content: '' })
      load()
    } catch (err) {
      setActionError(getErrorMessage(err, '添加跟进失败'))
    } finally {
      setAddingActivity(false)
    }
  }

  const addTask = async (e) => {
    e.preventDefault()
    if (!taskForm.title.trim() || !taskForm.due_at) return
    setAddingTask(true)
    setActionError('')
    try {
      await api.createTask(id, { title: taskForm.title, due_at: new Date(taskForm.due_at).toISOString() })
      setTaskForm({ title: '', due_at: '' })
      load()
    } catch (err) {
      setActionError(getErrorMessage(err, '创建待办失败'))
    } finally {
      setAddingTask(false)
    }
  }

  const openEdit = () => {
    setEditError('')
    setEditMode(true)
  }

  const updateCustomer = async (e) => {
    e.preventDefault()
    setEditError('')
    setSavingCustomer(true)
    try {
      const data = { ...editForm, stage: editForm.stage || null }
      await api.updateCustomer(id, data)
      setEditMode(false)
      load()
    } catch (err) {
      setEditError(getErrorMessage(err, '更新失败'))
    } finally {
      setSavingCustomer(false)
    }
  }

  const updateTaskStatus = async (taskId, status) => {
    setUpdatingTaskId(taskId)
    setActionError('')
    try {
      await api.updateTask(taskId, { status })
      load()
    } catch (err) {
      setActionError(getErrorMessage(err, '更新待办失败'))
    } finally {
      setUpdatingTaskId(null)
    }
  }

  // 换人（admin）：打开面板时拉取成员列表
  const openTransfer = async () => {
    setTransferMode(true)
    setTransferTo('')
    setTransferError('')
    try {
      const users = await api.listUsers()
      setTeamUsers(users.filter((u) => u.id !== customer?.owner_id))
    } catch (err) {
      setTransferError(getErrorMessage(err, '加载成员列表失败'))
    }
  }

  const doTransfer = async () => {
    if (!transferTo) return
    setTransferring(true)
    setTransferError('')
    try {
      await api.transferCustomer(id, transferTo)
      setTransferMode(false)
      load()
    } catch (err) {
      setTransferError(getErrorMessage(err, '换人失败'))
    } finally {
      setTransferring(false)
    }
  }

  // 逾期判定：pending 且截止时间已过（列表页逾期标红，spec §2.1）
  const isOverdue = (t) => t.status === 'pending' && new Date(t.due_at) < new Date()

  const stageName = (sid) => {
    const s = stages.find((st) => st.id === sid)
    return s ? s.name : '—'
  }

  const backButton = (
    <Button variant="ghost" onClick={() => navigate('/customers')} className="u-mb-3">
      ← 返回客户库
    </Button>
  )

  if (loading) {
    return (
      <div className="layout">
        <Sidebar navItems={getNavItems(user)} />
        <Page>
          {backButton}
          <CardSkeleton count={2} />
          <TableSkeleton rows={4} />
        </Page>
      </div>
    )
  }

  if (loadError) {
    return (
      <div className="layout">
        <Sidebar navItems={getNavItems(user)} />
        <Page>
          {backButton}
          <ErrorState message={loadError} onRetry={load} />
        </Page>
      </div>
    )
  }

  if (!customer) {
    return (
      <div className="layout">
        <Sidebar navItems={getNavItems(user)} />
        <Page>
          {backButton}
          <EmptyState
            title="客户不存在"
            description="该客户可能已被删除，或链接中的编号无效。"
            action={<Button variant="secondary" onClick={() => navigate('/customers')}>返回客户库</Button>}
          />
        </Page>
      </div>
    )
  }

  const actsAvailable = hasFeature('crm.activities') && !actsDenied
  const tasksAvailable = hasFeature('crm.tasks') && !tasksDenied

  return (
    <div className="layout">
      <Sidebar navItems={getNavItems(user)} />
      <Page>
        {backButton}

        <PageHeader
          title={customer.company}
          description={`客户主档 · ${stageName(customer.stage)}`}
          actions={(
            <>
              {isAdmin && (
                <Button variant="secondary" onClick={openTransfer}>换人</Button>
              )}
              <Button variant="secondary" onClick={openEdit}>编辑</Button>
            </>
          )}
        />

        {actionError && (
          <Callout tone="danger" actions={<Button size="sm" variant="ghost" onClick={() => setActionError('')}>关闭</Button>}>
            {actionError}
          </Callout>
        )}

        <Card title="客户信息" className="u-mb-4">
          <div className="meta-grid">
            <div className="meta-item">
              <span className="meta-label">联系人</span>
              <span className="meta-value">{customer.contact_name || '—'}</span>
            </div>
            <div className="meta-item">
              <span className="meta-label">电话</span>
              <span className="meta-value">{customer.phone || '—'}</span>
            </div>
            <div className="meta-item">
              <span className="meta-label">邮箱</span>
              <span className="meta-value">{customer.email || '—'}</span>
            </div>
            <div className="meta-item">
              <span className="meta-label">地址</span>
              <span className="meta-value">{customer.address || '—'}</span>
            </div>
            <div className="meta-item">
              <span className="meta-label">阶段</span>
              <span className="meta-value">{stageName(customer.stage)}</span>
            </div>
            <div className="meta-item">
              <span className="meta-label">来源</span>
              <span className="meta-value">{customer.source || '—'}</span>
            </div>
            <div className="meta-item">
              <span className="meta-label">创建时间</span>
              <span className="meta-value">{fmtDateTime(customer.created_at)}</span>
            </div>
            <div className="meta-item">
              <span className="meta-label">更新时间</span>
              <span className="meta-value">{fmtDateTime(customer.updated_at)}</span>
            </div>
          </div>
          {customer.notes && <div className="meta-notes">{customer.notes}</div>}
        </Card>

        <div className="content-grid">
          {/* 时间轴（功能分级 crm.activities：无授权或后端 403 时隐藏卡片，不拖垮整页） */}
          {actsAvailable ? (
            <Card title="跟进时间轴" subtitle={`${activities.length} 条记录`}>
              <form onSubmit={addActivity} className="u-mb-4">
                <div className="u-row u-gap-2 u-mb-2">
                  <Select
                    className="filter-select"
                    value={activityForm.type}
                    onChange={(e) => setActivityForm({ ...activityForm, type: e.target.value })}
                    aria-label="跟进方式"
                  >
                    <option value="call">电话</option>
                    <option value="email">邮件</option>
                    <option value="meeting">会议</option>
                    <option value="note">备注</option>
                    <option value="other">其他</option>
                  </Select>
                  <Input
                    className="u-flex-1"
                    placeholder="记录跟进内容…"
                    value={activityForm.content}
                    onChange={(e) => setActivityForm({ ...activityForm, content: e.target.value })}
                    aria-label="跟进内容"
                  />
                </div>
                <Button type="submit" variant="primary" size="sm" loading={addingActivity}>添加跟进</Button>
              </form>
              {actsError ? (
                <ErrorState
                  title="跟进记录加载失败"
                  message={actsError}
                  onRetry={load}
                  className="u-mt-3"
                />
              ) : activities.length === 0 ? (
                <EmptyState
                  inline
                  title="暂无跟进记录"
                  description="每次电话、邮件或会议后在上方记录一条，形成完整客户时间线。"
                />
              ) : (
                <div className="timeline">
                  {activities.map((a) => {
                    const meta = ACTIVITY_META[a.type] || ACTIVITY_META.other
                    return (
                      <div key={a.id} className="timeline-item">
                        <div className="timeline-dot" />
                        <div className="timeline-content">
                          <div className="timeline-header">
                            <Tag tone={meta.tone}>{meta.label}</Tag>
                            {a.author_name && <span className="timeline-author">{a.author_name}</span>}
                            <span className="timeline-date">{fmtDateTime(a.occurred_at)}</span>
                          </div>
                          <div className="timeline-text">{a.content}</div>
                        </div>
                      </div>
                    )
                  })}
                </div>
              )}
            </Card>
          ) : (
            <FeatureNotice />
          )}

          {/* 待办（功能分级 crm.tasks：无授权或后端 403 时隐藏卡片，不拖垮整页） */}
          {tasksAvailable ? (
            <Card title="待办任务" subtitle={`${tasks.length} 条记录`}>
              <form onSubmit={addTask} className="u-mb-4">
                <div className="u-col u-gap-2 u-mb-2">
                  <Input
                    placeholder="待办标题…"
                    value={taskForm.title}
                    onChange={(e) => setTaskForm({ ...taskForm, title: e.target.value })}
                    aria-label="待办标题"
                  />
                  <Input
                    type="datetime-local"
                    value={taskForm.due_at}
                    onChange={(e) => setTaskForm({ ...taskForm, due_at: e.target.value })}
                    aria-label="截止时间"
                  />
                </div>
                <Button type="submit" variant="primary" size="sm" loading={addingTask}>创建待办</Button>
              </form>
              {tasksError ? (
                <ErrorState
                  title="待办加载失败"
                  message={tasksError}
                  onRetry={load}
                  className="u-mt-3"
                />
              ) : tasks.length === 0 ? (
                <EmptyState
                  inline
                  title="暂无待办"
                  description="为这个客户创建下一次跟进提醒，到点会在待办列表中标红。"
                />
              ) : (
                <div className="task-list">
                  {tasks.map((t) => (
                    <div key={t.id} className={`task-item${isOverdue(t) ? ' is-overdue' : ''}`}>
                      <div className="task-main">
                        <div className="task-title">{t.title}</div>
                        <div className={`task-due${isOverdue(t) ? ' task-due--overdue' : ''}`}>
                          截止：{fmtDateTime(t.due_at)}
                          {isOverdue(t) && <Tag tone="danger">已逾期</Tag>}
                        </div>
                      </div>
                      <div className="task-actions">
                        {t.status === 'pending' && (
                          <Button
                            size="sm"
                            variant="secondary"
                            loading={updatingTaskId === t.id}
                            onClick={() => updateTaskStatus(t.id, 'done')}
                          >
                            完成
                          </Button>
                        )}
                        {t.status === 'pending' && (
                          <Button
                            size="sm"
                            variant="ghost"
                            disabled={updatingTaskId === t.id}
                            onClick={() => updateTaskStatus(t.id, 'cancelled')}
                          >
                            取消
                          </Button>
                        )}
                        {t.status === 'done' && <Tag tone="success">已完成</Tag>}
                        {t.status === 'cancelled' && <Tag tone="muted">已取消</Tag>}
                      </div>
                    </div>
                  ))}
                </div>
              )}
            </Card>
          ) : (
            <FeatureNotice />
          )}
        </div>

        <Modal
          open={editMode}
          onClose={() => setEditMode(false)}
          title="编辑客户"
          description={customer.company}
          size="lg"
          footer={(
            <>
              <Button variant="secondary" onClick={() => setEditMode(false)} disabled={savingCustomer}>取消</Button>
              <Button variant="primary" type="submit" form="customer-edit-form" loading={savingCustomer}>保存</Button>
            </>
          )}
        >
          {editError && <Callout tone="danger">{editError}</Callout>}
          <form id="customer-edit-form" onSubmit={updateCustomer}>
            <Field label="公司名" required htmlFor="edit-company">
              <Input
                id="edit-company"
                value={editForm.company || ''}
                onChange={(e) => setEditForm({ ...editForm, company: e.target.value })}
              />
            </Field>
            <div className="content-grid">
              <Field label="联系人" htmlFor="edit-contact">
                <Input
                  id="edit-contact"
                  value={editForm.contact_name || ''}
                  onChange={(e) => setEditForm({ ...editForm, contact_name: e.target.value })}
                />
              </Field>
              <Field label="电话" htmlFor="edit-phone">
                <Input
                  id="edit-phone"
                  value={editForm.phone || ''}
                  onChange={(e) => setEditForm({ ...editForm, phone: e.target.value })}
                />
              </Field>
              <Field label="邮箱" htmlFor="edit-email">
                <Input
                  id="edit-email"
                  type="email"
                  value={editForm.email || ''}
                  onChange={(e) => setEditForm({ ...editForm, email: e.target.value })}
                />
              </Field>
              <Field label="地址" htmlFor="edit-address">
                <Input
                  id="edit-address"
                  value={editForm.address || ''}
                  onChange={(e) => setEditForm({ ...editForm, address: e.target.value })}
                />
              </Field>
            </div>
            <Field label="销售阶段" htmlFor="edit-stage">
              <Select
                id="edit-stage"
                value={editForm.stage || ''}
                onChange={(e) => setEditForm({ ...editForm, stage: e.target.value })}
              >
                <option value="">未设置</option>
                {stages.map((s) => (
                  <option key={s.id} value={s.id}>{s.name}</option>
                ))}
              </Select>
            </Field>
            <Field label="备注" htmlFor="edit-notes">
              <Textarea
                id="edit-notes"
                rows={3}
                value={editForm.notes || ''}
                onChange={(e) => setEditForm({ ...editForm, notes: e.target.value })}
              />
            </Field>
          </form>
        </Modal>

        <Modal
          open={transferMode}
          onClose={() => setTransferMode(false)}
          title="负责人变更"
          description="历史跟进时间线完整保留，新负责人可看到全部历史记录；换人会自动在时间线上留一条交接记录。"
          footer={(
            <>
              <Button variant="secondary" onClick={() => setTransferMode(false)} disabled={transferring}>取消</Button>
              <Button
                variant="primary"
                onClick={doTransfer}
                loading={transferring}
                disabled={!transferTo}
              >
                确认换人
              </Button>
            </>
          )}
        >
          {transferError && <Callout tone="danger">{transferError}</Callout>}
          <Field label="新负责人" required htmlFor="transfer-owner">
            <Select
              id="transfer-owner"
              value={transferTo}
              onChange={(e) => setTransferTo(e.target.value)}
            >
              <option value="">请选择成员</option>
              {teamUsers.map((u) => (
                <option key={u.id} value={u.id}>{u.display_name || u.username}</option>
              ))}
            </Select>
          </Field>
        </Modal>
      </Page>
    </div>
  )
}
