import { useEffect, useState } from 'react'
import * as api from '../api'
import { Page, Sidebar, getNavItems } from '../components/Layout'
import {
  Button, Callout, Card, EmptyState, ErrorState, Field, Input, ListSkeleton, Modal,
  PageHeader, Select, Tag, fmtDateTime,
} from '../components/ui'
import { getErrorMessage } from '../api/client'
import { useAuth } from '../auth'

export default function Tasks() {
  const { user } = useAuth()
  const isAdmin = user?.role === 'admin'
  const [tasks, setTasks] = useState([])
  const [loading, setLoading] = useState(true)
  const [error, setError] = useState('')
  const [customers, setCustomers] = useState([])
  const [showForm, setShowForm] = useState(false)
  const [form, setForm] = useState({ title: '', due_at: '', customer_id: '' })
  const [formError, setFormError] = useState('')
  const [submitting, setSubmitting] = useState(false)
  const [busyId, setBusyId] = useState(null)

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
    setFormError('')
    setError('')
    try {
      const data = await api.listCustomers()
      setCustomers(data)
    } catch (err) {
      // 弹窗内展示：客户列表拉不到就选不到关联客户，不能静默
      setFormError(getErrorMessage(err, '加载客户列表失败'))
    }
  }

  const cancelForm = () => {
    setShowForm(false)
    setForm({ title: '', due_at: '', customer_id: '' })
    setFormError('')
  }

  const submitCreate = async () => {
    if (!form.title.trim()) { setFormError('请填写待办标题'); return }
    if (!form.due_at) { setFormError('请选择截止时间'); return }
    if (!form.customer_id) { setFormError('请选择关联客户'); return }
    setSubmitting(true)
    setFormError('')
    try {
      // datetime-local 值视为本地时间，转 RFC3339 传给后端
      const dueAt = new Date(form.due_at).toISOString()
      await api.createTask(form.customer_id, { title: form.title.trim(), due_at: dueAt })
      cancelForm()
      load()
    } catch (err) {
      setFormError(getErrorMessage(err, '新建待办失败'))
    } finally {
      setSubmitting(false)
    }
  }

  const updateStatus = async (id, status) => {
    setBusyId(id)
    try {
      await api.updateTask(id, { status })
      load()
    } catch (err) {
      setError(getErrorMessage(err, '更新待办失败'))
    } finally {
      setBusyId(null)
    }
  }

  const remove = async (id) => {
    setBusyId(id)
    try {
      await api.deleteTask(id)
      load()
    } catch (err) {
      setError(getErrorMessage(err, '删除待办失败'))
    } finally {
      setBusyId(null)
    }
  }

  const pending = tasks.filter((t) => t.status === 'pending')
  const done = tasks.filter((t) => t.status === 'done')
  const cancelled = tasks.filter((t) => t.status === 'cancelled')

  // 逾期判定：pending 且截止时间已过（列表页逾期标红，spec §2.1）
  const isOverdue = (t) => t.status === 'pending' && new Date(t.due_at) < new Date()

  // 任务副标题行：客户公司 +（admin）负责人
  const taskMeta = (t) => {
    const parts = []
    if (t.customer_company) parts.push(t.customer_company)
    if (isAdmin && t.assignee_name) parts.push(`负责人：${t.assignee_name}`)
    return parts.length > 0 ? <div className="task-meta">{parts.join(' · ')}</div> : null
  }

  return (
    <div className="layout">
      <Sidebar navItems={getNavItems(user)} />
      <Page>
        <PageHeader
          title="我的待办"
          description="按状态分组；逾期任务标红，完成后可恢复"
          actions={<Button variant="primary" onClick={openForm}>新建待办</Button>}
        />

        {error && tasks.length === 0 ? (
          <ErrorState message={error} onRetry={load} />
        ) : (
          <>
            {error && (
              <Callout tone="danger" actions={<Button size="sm" variant="secondary" onClick={load}>重试</Button>}>
                {error}
              </Callout>
            )}

            {loading ? (
              <Card title="待处理">
                <ListSkeleton rows={4} />
              </Card>
            ) : tasks.length === 0 ? (
              <Card>
                <EmptyState
                  title="暂无待办任务"
                  description="为某个客户创建跟进提醒，到点后会出现在这里并按截止时间标红。"
                  action={<Button variant="primary" onClick={openForm}>+ 新建待办</Button>}
                />
              </Card>
            ) : (
              <>
                <Card title={`待处理（${pending.length}）`} className="u-mb-4">
                  {pending.length === 0 ? (
                    <EmptyState inline title="没有待处理任务" description="全部待办都已完成或已取消。" />
                  ) : (
                    <div className="task-list">
                      {pending.map((t) => (
                        <div key={t.id} className={`task-item${isOverdue(t) ? ' is-overdue' : ''}`}>
                          <div className="task-main">
                            <div className="task-title">{t.title}</div>
                            {taskMeta(t)}
                            <div className={`task-due${isOverdue(t) ? ' task-due--overdue' : ''}`}>
                              截止：{fmtDateTime(t.due_at)}
                              {isOverdue(t) && <Tag tone="danger">已逾期</Tag>}
                            </div>
                          </div>
                          <div className="task-actions">
                            <Button
                              size="sm"
                              variant="secondary"
                              loading={busyId === t.id}
                              onClick={() => updateStatus(t.id, 'done')}
                            >
                              完成
                            </Button>
                            <Button
                              size="sm"
                              variant="ghost"
                              disabled={busyId === t.id}
                              onClick={() => updateStatus(t.id, 'cancelled')}
                            >
                              取消
                            </Button>
                            <Button
                              size="sm"
                              variant="danger"
                              disabled={busyId === t.id}
                              onClick={() => remove(t.id)}
                            >
                              删除
                            </Button>
                          </div>
                        </div>
                      ))}
                    </div>
                  )}
                </Card>

                {done.length > 0 && (
                  <Card title={`已完成（${done.length}）`} className="u-mb-4">
                    <div className="task-list">
                      {done.map((t) => (
                        <div key={t.id} className="task-item">
                          <div className="task-main">
                            <div className="task-title task-title--done">{t.title}</div>
                            {taskMeta(t)}
                            <div className="task-due">截止：{fmtDateTime(t.due_at)}</div>
                          </div>
                          <div className="task-actions">
                            <Tag tone="success">已完成</Tag>
                            <Button
                              size="sm"
                              variant="secondary"
                              disabled={busyId === t.id}
                              onClick={() => updateStatus(t.id, 'pending')}
                            >
                              恢复
                            </Button>
                            <Button
                              size="sm"
                              variant="danger"
                              disabled={busyId === t.id}
                              onClick={() => remove(t.id)}
                            >
                              删除
                            </Button>
                          </div>
                        </div>
                      ))}
                    </div>
                  </Card>
                )}

                {cancelled.length > 0 && (
                  <Card title={`已取消（${cancelled.length}）`}>
                    <div className="task-list">
                      {cancelled.map((t) => (
                        <div key={t.id} className="task-item">
                          <div className="task-main">
                            <div className="task-title task-title--cancelled">{t.title}</div>
                            {taskMeta(t)}
                            <div className="task-due">截止：{fmtDateTime(t.due_at)}</div>
                          </div>
                          <div className="task-actions">
                            <Tag tone="muted">已取消</Tag>
                            <Button
                              size="sm"
                              variant="danger"
                              disabled={busyId === t.id}
                              onClick={() => remove(t.id)}
                            >
                              删除
                            </Button>
                          </div>
                        </div>
                      ))}
                    </div>
                  </Card>
                )}
              </>
            )}
          </>
        )}

        <Modal
          open={showForm}
          onClose={cancelForm}
          title="新建待办"
          description="到点后会在「我的待办」与客户详情待办卡片中提醒"
          footer={(
            <>
              <Button variant="secondary" onClick={cancelForm} disabled={submitting}>取消</Button>
              <Button variant="primary" type="submit" form="task-create-form" loading={submitting}>创建</Button>
            </>
          )}
        >
          {formError && <Callout tone="danger">{formError}</Callout>}
          <form id="task-create-form" onSubmit={(e) => { e.preventDefault(); submitCreate() }}>
            <Field label="标题" required htmlFor="task-title">
              <Input
                id="task-title"
                type="text"
                value={form.title}
                onChange={(e) => setForm({ ...form, title: e.target.value })}
                placeholder="例如：回访客户"
              />
            </Field>
            <Field label="截止时间" required htmlFor="task-due">
              <Input
                id="task-due"
                type="datetime-local"
                value={form.due_at}
                onChange={(e) => setForm({ ...form, due_at: e.target.value })}
              />
            </Field>
            <Field label="关联客户" required htmlFor="task-customer">
              <Select
                id="task-customer"
                value={form.customer_id}
                onChange={(e) => setForm({ ...form, customer_id: e.target.value })}
              >
                <option value="">请选择客户</option>
                {customers.map((c) => (
                  <option key={c.id} value={c.id}>
                    {c.company}{c.contact_name ? `（${c.contact_name}）` : ''}
                  </option>
                ))}
              </Select>
            </Field>
          </form>
        </Modal>
      </Page>
    </div>
  )
}
