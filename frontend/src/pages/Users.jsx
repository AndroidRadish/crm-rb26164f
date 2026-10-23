import { useEffect, useState } from 'react'
import * as api from '../api'
import { Page, Sidebar, getNavItems } from '../components/Layout'
import {
  Button, Callout, EmptyState, ErrorState, Field, Input, Modal, PageHeader, Select,
  Table, TableSkeleton, Tag, fmtDateTime,
} from '../components/ui'
import { getErrorMessage } from '../api/client'
import { useAuth } from '../auth'

const EMPTY_FORM = { username: '', password: '', role: 'member', display_name: '' }

export default function Users() {
  const { user } = useAuth()
  const [users, setUsers] = useState([])
  const [loading, setLoading] = useState(true)
  const [error, setError] = useState('')
  const [showForm, setShowForm] = useState(false)
  const [form, setForm] = useState(EMPTY_FORM)
  const [formError, setFormError] = useState('')
  const [submitting, setSubmitting] = useState(false)
  // 成员生命周期（停用/启用/重置密码）
  const [actionError, setActionError] = useState('')
  const [busyId, setBusyId] = useState(null)
  const [resetTarget, setResetTarget] = useState(null)
  const [resetPassword, setResetPassword] = useState('')
  const [resetError, setResetError] = useState('')
  const [resetSubmitting, setResetSubmitting] = useState(false)

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

  const openForm = () => {
    setFormError('')
    setShowForm(true)
  }

  const closeForm = () => {
    setShowForm(false)
    setFormError('')
  }

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
    setSubmitting(true)
    try {
      await api.createUser(form.username, form.password, form.role, form.display_name)
      setForm(EMPTY_FORM)
      setShowForm(false)
      load()
    } catch (err) {
      setFormError(getErrorMessage(err, '创建成员失败'))
    } finally {
      setSubmitting(false)
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
    setResetSubmitting(true)
    try {
      await api.resetUserPassword(resetTarget.id, resetPassword)
      setResetTarget(null)
      setResetPassword('')
      load()
    } catch (err) {
      setResetError(getErrorMessage(err, '重置密码失败'))
    } finally {
      setResetSubmitting(false)
    }
  }

  return (
    <div className="layout">
      <Sidebar navItems={getNavItems(user)} />
      <Page>
        <PageHeader
          title="成员管理"
          description="新增成员、停用/启用账号与重置密码"
          actions={<Button variant="primary" onClick={openForm}>+ 新增成员</Button>}
        />

        {actionError && <Callout tone="danger">{actionError}</Callout>}

        {error && users.length === 0 ? (
          <ErrorState message={error} onRetry={load} />
        ) : (
          <>
            {error && (
              <Callout tone="danger" actions={<Button size="sm" variant="secondary" onClick={load}>重试</Button>}>
                {error}
              </Callout>
            )}

            {loading ? (
              <TableSkeleton rows={4} />
            ) : users.length === 0 ? (
              <EmptyState
                title="暂无成员"
                description="创建成员账号后，成员可登录 CRM 处理自己名下的客户与待办。"
                action={<Button variant="primary" onClick={openForm}>+ 新增成员</Button>}
              />
            ) : (
              <Table columns={['用户名', '显示名称', '角色', '状态', '创建时间', '操作']}>
                {users.map((u) => (
                  <tr key={u.id}>
                    <td>
                      <div className="u-row u-gap-2">
                        <span>{u.username}</span>
                        {user && u.id === user.id && <Tag tone="accent">我</Tag>}
                      </div>
                    </td>
                    <td>{u.display_name || '—'}</td>
                    <td>
                      {u.role === 'admin'
                        ? <Tag tone="success">管理员</Tag>
                        : <Tag tone="neutral">成员</Tag>}
                    </td>
                    <td>
                      {u.is_active
                        ? <Tag tone="accent">启用中</Tag>
                        : <Tag tone="muted">已停用</Tag>}
                    </td>
                    <td className="cell-muted">{fmtDateTime(u.created_at)}</td>
                    <td>
                      <div className="table-actions">
                        <Button
                          size="sm"
                          variant={u.is_active ? 'danger' : 'secondary'}
                          loading={busyId === u.id}
                          onClick={() => toggleActive(u)}
                        >
                          {busyId === u.id ? '处理中…' : u.is_active ? '停用' : '启用'}
                        </Button>
                        <Button
                          size="sm"
                          variant="secondary"
                          disabled={busyId === u.id}
                          onClick={() => openReset(u)}
                        >
                          重置密码
                        </Button>
                      </div>
                    </td>
                  </tr>
                ))}
              </Table>
            )}
          </>
        )}

        <Modal
          open={showForm}
          onClose={closeForm}
          title="新增成员"
          description="成员创建后即可登录；密码至少 6 位"
          footer={(
            <>
              <Button variant="secondary" onClick={closeForm} disabled={submitting}>取消</Button>
              <Button variant="primary" type="submit" form="user-create-form" loading={submitting}>创建</Button>
            </>
          )}
        >
          {formError && <Callout tone="danger">{formError}</Callout>}
          <form id="user-create-form" onSubmit={create}>
            <Field label="用户名" required htmlFor="new-username">
              <Input
                id="new-username"
                value={form.username}
                onChange={(e) => setForm({ ...form, username: e.target.value })}
              />
            </Field>
            <Field label="密码" required hint="至少 6 位" htmlFor="new-password">
              <Input
                id="new-password"
                type="password"
                value={form.password}
                onChange={(e) => setForm({ ...form, password: e.target.value })}
              />
            </Field>
            <Field label="显示名称" htmlFor="new-display-name">
              <Input
                id="new-display-name"
                value={form.display_name}
                onChange={(e) => setForm({ ...form, display_name: e.target.value })}
              />
            </Field>
            <Field label="角色" htmlFor="new-role">
              <Select
                id="new-role"
                value={form.role}
                onChange={(e) => setForm({ ...form, role: e.target.value })}
              >
                <option value="member">普通成员</option>
                <option value="admin">管理员</option>
              </Select>
            </Field>
          </form>
        </Modal>

        <Modal
          open={Boolean(resetTarget)}
          onClose={() => setResetTarget(null)}
          title={resetTarget ? `重置密码 — ${resetTarget.username}` : '重置密码'}
          description="管理员直接设置新密码，无需旧密码；重置后请把新密码线下告知成员。"
          footer={(
            <>
              <Button variant="secondary" onClick={() => setResetTarget(null)} disabled={resetSubmitting}>取消</Button>
              <Button variant="primary" type="submit" form="user-reset-form" loading={resetSubmitting}>确认重置</Button>
            </>
          )}
        >
          {resetError && <Callout tone="danger">{resetError}</Callout>}
          <form id="user-reset-form" onSubmit={submitReset}>
            <Field label="新密码" required hint="至少 6 位" htmlFor="reset-password">
              <Input
                id="reset-password"
                type="password"
                value={resetPassword}
                onChange={(e) => setResetPassword(e.target.value)}
              />
            </Field>
          </form>
        </Modal>
      </Page>
    </div>
  )
}
