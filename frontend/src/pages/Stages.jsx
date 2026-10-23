import { useEffect, useState } from 'react'
import * as api from '../api'
import { Page, Sidebar, getNavItems } from '../components/Layout'
import {
  Button, Callout, Checkbox, EmptyState, ErrorState, Field, Input, Modal, PageHeader,
  Table, TableSkeleton, Tag,
} from '../components/ui'
import { getErrorMessage } from '../api/client'
import { useAuth } from '../auth'

const EMPTY_FORM = { name: '', order_index: '', is_won: false, is_lost: false }

export default function Stages() {
  const { user } = useAuth()
  const [stages, setStages] = useState([])
  const [loading, setLoading] = useState(true)
  const [error, setError] = useState('')
  const [showForm, setShowForm] = useState(false)
  const [form, setForm] = useState(EMPTY_FORM)
  const [formError, setFormError] = useState('')
  const [submitting, setSubmitting] = useState(false)
  const [busyId, setBusyId] = useState(null)

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
    if (!form.name.trim()) { setFormError('请填写阶段名称'); return }
    setSubmitting(true)
    try {
      await api.createStage({
        name: form.name,
        order_index: form.order_index ? parseInt(form.order_index) : undefined,
        is_won: form.is_won,
        is_lost: form.is_lost,
      })
      setForm(EMPTY_FORM)
      setShowForm(false)
      load()
    } catch (err) {
      setFormError(getErrorMessage(err, '创建阶段失败'))
    } finally {
      setSubmitting(false)
    }
  }

  const update = async (id, data) => {
    setBusyId(id)
    try {
      await api.updateStage(id, data)
      load()
    } catch (err) {
      setError(getErrorMessage(err, '更新阶段失败'))
    } finally {
      setBusyId(null)
    }
  }

  const remove = async (id) => {
    if (!confirm('确定删除该阶段？')) return
    setBusyId(id)
    try {
      await api.deleteStage(id)
      load()
    } catch (err) {
      setError(getErrorMessage(err, '删除阶段失败'))
    } finally {
      setBusyId(null)
    }
  }

  return (
    <div className="layout">
      <Sidebar navItems={getNavItems(user)} />
      <Page>
        <PageHeader
          title="销售阶段管理"
          description="阶段顺序决定漏斗与看板列顺序；赢单/输单阶段参与成交率统计"
          actions={<Button variant="primary" onClick={openForm}>+ 新增阶段</Button>}
        />

        {error && stages.length === 0 ? (
          <ErrorState message={error} onRetry={load} />
        ) : (
          <>
            {error && (
              <Callout tone="danger" actions={<Button size="sm" variant="secondary" onClick={load}>重试</Button>}>
                {error}
              </Callout>
            )}

            {loading ? (
              <TableSkeleton rows={5} />
            ) : stages.length === 0 ? (
              <EmptyState
                title="暂无销售阶段"
                description="创建阶段后，客户可以按阶段推进，看板与漏斗才有数据。"
                action={<Button variant="primary" onClick={openForm}>+ 新增阶段</Button>}
              />
            ) : (
              <Table columns={['排序', '名称', '类型', '状态', '操作']}>
                {stages.map((s) => (
                  <tr key={s.id}>
                    <td className="cell-muted">{s.order_index}</td>
                    <td>{s.name}</td>
                    <td>
                      {s.is_won
                        ? <Tag tone="success">赢单</Tag>
                        : s.is_lost
                          ? <Tag tone="muted">输单</Tag>
                          : <Tag tone="info">进行中</Tag>}
                    </td>
                    <td>
                      {s.is_active
                        ? <Tag tone="accent">启用</Tag>
                        : <Tag tone="muted">停用</Tag>}
                    </td>
                    <td>
                      <div className="table-actions">
                        <Button
                          size="sm"
                          variant="ghost"
                          loading={busyId === s.id}
                          onClick={() => update(s.id, { is_active: !s.is_active })}
                        >
                          {s.is_active ? '停用' : '启用'}
                        </Button>
                        <Button
                          size="sm"
                          variant="danger"
                          disabled={busyId === s.id}
                          onClick={() => remove(s.id)}
                        >
                          删除
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
          title="新增销售阶段"
          description="排序数字越小越靠前；同一阶段不要同时标记赢单与输单"
          footer={(
            <>
              <Button variant="secondary" onClick={closeForm} disabled={submitting}>取消</Button>
              <Button variant="primary" type="submit" form="stage-create-form" loading={submitting}>创建</Button>
            </>
          )}
        >
          {formError && <Callout tone="danger">{formError}</Callout>}
          <form id="stage-create-form" onSubmit={create}>
            <Field label="阶段名称" required htmlFor="stage-name">
              <Input
                id="stage-name"
                value={form.name}
                onChange={(e) => setForm({ ...form, name: e.target.value })}
              />
            </Field>
            <Field label="排序" hint="数字越小越靠前" htmlFor="stage-order">
              <Input
                id="stage-order"
                type="number"
                value={form.order_index}
                onChange={(e) => setForm({ ...form, order_index: e.target.value })}
              />
            </Field>
            <div className="u-row u-gap-5">
              <Checkbox
                label="赢单"
                checked={form.is_won}
                onChange={(e) => setForm({ ...form, is_won: e.target.checked })}
              />
              <Checkbox
                label="输单"
                checked={form.is_lost}
                onChange={(e) => setForm({ ...form, is_lost: e.target.checked })}
              />
            </div>
          </form>
        </Modal>
      </Page>
    </div>
  )
}
