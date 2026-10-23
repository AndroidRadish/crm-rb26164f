import { useEffect, useState } from 'react'
import { useNavigate } from 'react-router-dom'
import * as api from '../api'
import { Page, Sidebar, getNavItems } from '../components/Layout'
import {
  Button, Callout, EmptyState, ErrorState, Field, Input, Modal, PageHeader, Select,
  Table, TableSkeleton, Tag, Textarea, fmtDate,
} from '../components/ui'
import { getErrorMessage } from '../api/client'
import { useAuth } from '../auth'

const EMPTY_FORM = {
  company: '', contact_name: '', phone: '', email: '', address: '', notes: '', stage: '',
}

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
  const [form, setForm] = useState(EMPTY_FORM)
  const [formError, setFormError] = useState('')
  const [dupWarning, setDupWarning] = useState(null)
  const [submitting, setSubmitting] = useState(false)
  const [busyId, setBusyId] = useState(null)

  // 统一按显式筛选条件取数：避免 select 变更时读到旧 state（原实现会滞后一次）
  const loadWith = async ({ search: s, stage, owner }) => {
    setLoading(true)
    try {
      const params = {}
      if (s) params.search = s
      if (stage) params.stage = stage
      if (owner) params.owner = owner
      const data = await api.listCustomers(params)
      setCustomers(data)
      setError('')
    } catch (err) {
      setError(getErrorMessage(err, '加载客户失败'))
    } finally {
      setLoading(false)
    }
  }

  const load = () => loadWith({ search, stage: stageFilter, owner: ownerFilter })

  const resetFilters = () => {
    setSearch('')
    setStageFilter('')
    setOwnerFilter('')
    loadWith({ search: '', stage: '', owner: '' })
  }

  const loadStages = async () => {
    try {
      const data = await api.listStages()
      setStages(data)
    } catch (err) {
      // 阶段只用于筛选/展示名称：失败时不阻断客户列表，但记录原因便于排查
      console.warn('[customers] 阶段列表加载失败，阶段列降级为「—」', err)
    }
  }

  useEffect(() => { load(); loadStages() }, [])

  const openCreate = () => {
    setFormError('')
    setDupWarning(null)
    setShowForm(true)
  }

  const closeCreate = () => {
    setShowForm(false)
    setFormError('')
    setDupWarning(null)
  }

  const create = async (e) => {
    e.preventDefault()
    setFormError('')
    setDupWarning(null)
    if (!form.company.trim()) { setFormError('请填写公司名'); return }

    setSubmitting(true)

    // 防撞单检测：辅助提示。接口失败不阻断创建，但记录原因（后端 create 另有 409 兜底）
    try {
      const dup = await api.checkDuplicate({ company: form.company, phone: form.phone })
      if (dup.is_duplicate) {
        setDupWarning(dup.duplicates || [])
        setSubmitting(false)
        return
      }
    } catch (err) {
      console.warn('[customers] 重复客户检查失败，继续创建', err)
    }

    try {
      const data = { ...form, stage: form.stage || null }
      await api.createCustomer(data)
      setForm(EMPTY_FORM)
      setShowForm(false)
      load()
    } catch (err) {
      // 预检查未命中但后端仍判重（并发创建）：按重复客户提示，不吞错
      if (err?.response?.status === 409) {
        setDupWarning(err.response.data?.duplicates || [])
        setFormError('公司名或电话与已有客户重复，请修改后再提交')
      } else {
        setFormError(getErrorMessage(err, '创建失败'))
      }
    } finally {
      setSubmitting(false)
    }
  }

  const claim = async (id) => {
    setBusyId(id)
    try {
      await api.claimCustomer(id)
      load()
    } catch (err) {
      setError(getErrorMessage(err, '认领失败'))
    } finally {
      setBusyId(null)
    }
  }

  const release = async (id) => {
    setBusyId(id)
    try {
      await api.releaseCustomer(id)
      load()
    } catch (err) {
      setError(getErrorMessage(err, '释放失败'))
    } finally {
      setBusyId(null)
    }
  }

  const stageName = (id) => {
    const s = stages.find((st) => st.id === id)
    return s ? s.name : '—'
  }

  const hasFilter = Boolean(search || stageFilter || ownerFilter)

  return (
    <div className="layout">
      <Sidebar navItems={getNavItems(user)} />
      <Page>
        <PageHeader
          title="客户库"
          description="全部客户与归属状态；点击行进入详情与跟进时间线"
          actions={<Button variant="primary" onClick={openCreate}>+ 新增客户</Button>}
        />

        <div className="filter-bar">
          <Input
            className="filter-search"
            placeholder="搜索公司 / 联系人 / 电话…"
            value={search}
            onChange={(e) => setSearch(e.target.value)}
            onKeyDown={(e) => e.key === 'Enter' && load()}
            aria-label="搜索客户"
          />
          <Select
            className="filter-select"
            value={stageFilter}
            onChange={(e) => {
              setStageFilter(e.target.value)
              loadWith({ search, stage: e.target.value, owner: ownerFilter })
            }}
            aria-label="按阶段筛选"
          >
            <option value="">全部阶段</option>
            {stages.map((s) => (
              <option key={s.id} value={s.id}>{s.name}</option>
            ))}
          </Select>
          <Select
            className="filter-select"
            value={ownerFilter}
            onChange={(e) => {
              setOwnerFilter(e.target.value)
              loadWith({ search, stage: stageFilter, owner: e.target.value })
            }}
            aria-label="按归属筛选"
          >
            <option value="">全部归属</option>
            <option value="me">我的客户</option>
            <option value="pool">公海池</option>
          </Select>
          <Button variant="secondary" onClick={load}>查询</Button>
          {hasFilter && <Button variant="ghost" onClick={resetFilters}>清除筛选</Button>}
        </div>

        {error && customers.length === 0 ? (
          <ErrorState message={error} onRetry={load} />
        ) : (
          <>
            {error && (
              <Callout tone="danger" actions={<Button size="sm" variant="secondary" onClick={load}>重试</Button>}>
                {error}
              </Callout>
            )}

            {loading ? (
              <TableSkeleton rows={6} />
            ) : customers.length === 0 ? (
              hasFilter ? (
                <EmptyState
                  title="没有匹配的客户"
                  description="当前搜索/筛选条件下没有客户，试试调整关键词或清除筛选。"
                  action={<Button variant="secondary" onClick={resetFilters}>清除筛选</Button>}
                />
              ) : (
                <EmptyState
                  title="暂无客户"
                  description="新增第一位客户，或从数据导入批量导入。"
                  action={<Button variant="primary" onClick={openCreate}>+ 新增客户</Button>}
                />
              )
            ) : (
              <Table columns={['公司名', '联系人', '电话', '阶段', '归属', '更新时间', '操作']}>
                {customers.map((c) => (
                  <tr key={c.id} className="is-clickable" onClick={() => navigate(`/customers/${c.id}`)}>
                    <td>{c.company}</td>
                    <td>{c.contact_name || '—'}</td>
                    <td>{c.phone || '—'}</td>
                    <td><Tag tone="neutral">{stageName(c.stage)}</Tag></td>
                    <td>{c.owner_id ? (c.owner_id === user?.id ? '我' : '他人') : '公海'}</td>
                    <td className="cell-muted">{fmtDate(c.updated_at)}</td>
                    <td onClick={(e) => e.stopPropagation()}>
                      <div className="table-actions">
                        {!c.owner_id && canPool && (
                          <Button
                            size="sm"
                            variant="secondary"
                            loading={busyId === c.id}
                            onClick={() => claim(c.id)}
                          >
                            认领
                          </Button>
                        )}
                        {c.owner_id === user?.id && canPool && (
                          <Button
                            size="sm"
                            variant="ghost"
                            loading={busyId === c.id}
                            onClick={() => release(c.id)}
                          >
                            释放
                          </Button>
                        )}
                        {!((!c.owner_id || c.owner_id === user?.id) && canPool) && (
                          <span className="cell-muted" title="仅可认领公海客户或释放在自己名下的客户">—</span>
                        )}
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
          onClose={closeCreate}
          title="新增客户"
          description="创建后默认归属当前登录成员；重复公司名/电话会被后端拦截"
          size="lg"
          footer={(
            <>
              <Button variant="secondary" onClick={closeCreate} disabled={submitting}>取消</Button>
              <Button variant="primary" type="submit" form="customer-create-form" loading={submitting}>
                创建
              </Button>
            </>
          )}
        >
          {formError && <Callout tone="danger">{formError}</Callout>}
          {dupWarning && dupWarning.length > 0 && (
            <Callout
              tone="warning"
              title="检测到疑似重复客户"
              actions={<Button size="sm" variant="secondary" onClick={() => setDupWarning(null)}>关闭提示</Button>}
            >
              <div className="callout-list">
                {dupWarning.map((d) => (
                  <div key={d.id}>
                    {d.company} · {d.contact_name || '—'} · {d.phone || '—'}
                  </div>
                ))}
              </div>
              请修改公司名或电话后重新提交；已有客户可在客户库中搜索查看。
            </Callout>
          )}
          <form id="customer-create-form" onSubmit={create}>
            <Field label="公司名" required htmlFor="customer-company">
              <Input
                id="customer-company"
                value={form.company}
                onChange={(e) => setForm({ ...form, company: e.target.value })}
              />
            </Field>
            <div className="content-grid">
              <Field label="联系人" htmlFor="customer-contact">
                <Input
                  id="customer-contact"
                  value={form.contact_name}
                  onChange={(e) => setForm({ ...form, contact_name: e.target.value })}
                />
              </Field>
              <Field label="电话" htmlFor="customer-phone">
                <Input
                  id="customer-phone"
                  value={form.phone}
                  onChange={(e) => setForm({ ...form, phone: e.target.value })}
                />
              </Field>
              <Field label="邮箱" htmlFor="customer-email">
                <Input
                  id="customer-email"
                  type="email"
                  value={form.email}
                  onChange={(e) => setForm({ ...form, email: e.target.value })}
                />
              </Field>
              <Field label="地址" htmlFor="customer-address">
                <Input
                  id="customer-address"
                  value={form.address}
                  onChange={(e) => setForm({ ...form, address: e.target.value })}
                />
              </Field>
            </div>
            <Field label="销售阶段" htmlFor="customer-stage">
              <Select
                id="customer-stage"
                value={form.stage}
                onChange={(e) => setForm({ ...form, stage: e.target.value })}
              >
                <option value="">未设置</option>
                {stages.map((s) => (
                  <option key={s.id} value={s.id}>{s.name}</option>
                ))}
              </Select>
            </Field>
            <Field label="备注" htmlFor="customer-notes">
              <Textarea
                id="customer-notes"
                rows={3}
                value={form.notes}
                onChange={(e) => setForm({ ...form, notes: e.target.value })}
              />
            </Field>
          </form>
        </Modal>
      </Page>
    </div>
  )
}
