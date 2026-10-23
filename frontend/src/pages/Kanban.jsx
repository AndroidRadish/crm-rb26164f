import { useEffect, useState } from 'react'
import { useNavigate } from 'react-router-dom'
import * as api from '../api'
import { Page, Sidebar, getNavItems } from '../components/Layout'
import {
  BoardSkeleton, Button, Callout, EmptyState, ErrorState, PageHeader, Tag, cx,
} from '../components/ui'
import { getErrorMessage } from '../api/client'
import { useAuth } from '../auth'

export default function Kanban() {
  const navigate = useNavigate()
  const { user, hasFeature } = useAuth()
  const isAdmin = user?.role === 'admin'
  const canPool = hasFeature('pool.claim')
  const [columns, setColumns] = useState([])
  const [loading, setLoading] = useState(true)
  const [error, setError] = useState('')
  const [dragStageId, setDragStageId] = useState(null)
  const [draggingCardId, setDraggingCardId] = useState(null)
  const [dragOverStageId, setDragOverStageId] = useState(null)
  const [pendingDrop, setPendingDrop] = useState(null) // { customerId, targetStageId }
  const [busyId, setBusyId] = useState(null) // 正在操作的客户 id（防重复提交）

  const load = async () => {
    setLoading(true)
    try {
      const data = await api.getKanban()
      setColumns(data)
      setError('')
    } catch (err) {
      setError(getErrorMessage(err, '加载看板失败'))
    } finally {
      setLoading(false)
    }
  }

  useEffect(() => { load() }, [])

  // 判断卡片是否属于当前用户
  const isMine = (ownerId) => {
    if (!ownerId || !user) return false
    return String(ownerId) === String(user.id)
  }

  // 认领公海客户
  const handleClaim = async (e, customerId) => {
    e.stopPropagation()
    if (busyId) return
    setBusyId(customerId)
    setError('')
    try {
      await api.claimCustomer(customerId)
      await load()
    } catch (err) {
      setError(getErrorMessage(err, '认领失败'))
    } finally {
      setBusyId(null)
    }
  }

  // 释放回公海
  const handleRelease = async (e, customerId) => {
    e.stopPropagation()
    if (busyId) return
    if (!window.confirm('确定将该客户释放回公海池吗？')) return
    setBusyId(customerId)
    setError('')
    try {
      await api.releaseCustomer(customerId)
      await load()
    } catch (err) {
      setError(getErrorMessage(err, '释放失败'))
    } finally {
      setBusyId(null)
    }
  }

  const handleDragStart = (e, customerId, stageId) => {
    e.dataTransfer.setData('text/plain', customerId)
    e.dataTransfer.effectAllowed = 'move'
    setDragStageId(stageId)
    setDraggingCardId(customerId)
  }

  const handleDragEnd = () => {
    setDragStageId(null)
    setDraggingCardId(null)
    setDragOverStageId(null)
  }

  const handleDrop = (e, targetStageId) => {
    e.preventDefault()
    const customerId = e.dataTransfer.getData('text/plain')
    setDragOverStageId(null)
    // 同列投放不触发确认、不调用 API
    if (!customerId || dragStageId === targetStageId) {
      setDragStageId(null)
      return
    }
    // 弹确认，确认后才调用 API
    if (!window.confirm('确定将该客户移动到该阶段吗？')) {
      setDragStageId(null)
      return
    }
    setPendingDrop({ customerId, targetStageId })
    setDragStageId(null)
  }

  // 执行移阶段
  useEffect(() => {
    if (!pendingDrop) return
    const { customerId, targetStageId } = pendingDrop
    setPendingDrop(null)
    setBusyId(customerId)
    setError('')
    ;(async () => {
      try {
        await api.updateCustomer(customerId, { stage: targetStageId })
        await load()
      } catch (err) {
        setError(getErrorMessage(err, '移动客户失败'))
      } finally {
        setBusyId(null)
      }
    })()
  }, [pendingDrop])

  const handleDragOver = (e, stageId) => {
    e.preventDefault()
    e.dataTransfer.dropEffect = 'move'
    if (dragOverStageId !== stageId) setDragOverStageId(stageId)
  }

  return (
    <div className="layout">
      <Sidebar navItems={getNavItems(user)} />
      <Page>
        <PageHeader
          title="销售看板"
          description="拖拽卡片切换销售阶段；点击卡片进入客户详情"
          actions={<Button variant="secondary" onClick={load} disabled={loading}>刷新</Button>}
        />

        {loading ? (
          <BoardSkeleton columns={4} />
        ) : error && columns.length === 0 ? (
          <ErrorState message={error} onRetry={load} />
        ) : columns.length === 0 ? (
          <EmptyState
            title="暂无销售阶段"
            description={isAdmin
              ? '先到「销售阶段」创建阶段，客户看板按阶段分列展示。'
              : '管理员还未配置销售阶段；配置后客户会按阶段分列展示。'}
            action={isAdmin
              ? <Button variant="primary" onClick={() => navigate('/stages')}>去销售阶段</Button>
              : undefined}
          />
        ) : (
          <>
            {error && (
              <Callout tone="danger" actions={<Button size="sm" variant="secondary" onClick={load}>重试</Button>}>
                {error}
              </Callout>
            )}
            <div className="kanban-board">
              {columns.map((col) => (
                <div
                  key={col.stage_id}
                  className={cx('kanban-column', dragOverStageId === col.stage_id && 'is-dragover')}
                  onDragOver={(e) => handleDragOver(e, col.stage_id)}
                  onDragLeave={(e) => {
                    if (!e.currentTarget.contains(e.relatedTarget)) setDragOverStageId(null)
                  }}
                  onDrop={(e) => handleDrop(e, col.stage_id)}
                >
                  <div className="kanban-column-header">
                    <span className="kanban-column-name" title={col.stage_name}>{col.stage_name}</span>
                    <span className="kanban-column-count">{col.customers.length}</span>
                  </div>
                  <div className="kanban-column-body">
                    {col.customers.length === 0 ? (
                      <div className="kanban-empty">拖拽客户到此</div>
                    ) : (
                      col.customers.map((c) => (
                        <div
                          key={c.id}
                          className={cx('kanban-card', draggingCardId === c.id && 'is-dragging')}
                          draggable={!busyId}
                          onDragStart={(e) => handleDragStart(e, c.id, col.stage_id)}
                          onDragEnd={handleDragEnd}
                          onClick={() => navigate(`/customers/${c.id}`)}
                        >
                          <div className="kanban-card-company" title={c.company}>{c.company}</div>
                          <div
                            className="kanban-card-contact"
                            title={(c.contact_name || '—') + (c.phone ? ` · ${c.phone}` : '')}
                          >
                            {c.contact_name || '—'} {c.phone ? `· ${c.phone}` : ''}
                          </div>
                          <div className="kanban-card-owner">
                            {c.owner_id ? (
                              isMine(c.owner_id)
                                ? <Tag tone="accent">负责人：我</Tag>
                                : <Tag tone="neutral">负责人：{c.owner_name || '他人'}</Tag>
                            ) : (
                              <Tag tone="muted">公海</Tag>
                            )}
                          </div>
                          <div className="kanban-card-actions">
                            {!c.owner_id ? (
                              canPool && (
                                <Button
                                  size="sm"
                                  variant="primary"
                                  loading={busyId === c.id}
                                  onClick={(e) => handleClaim(e, c.id)}
                                >
                                  {busyId === c.id ? '认领中…' : '加入手头'}
                                </Button>
                              )
                            ) : isMine(c.owner_id) ? (
                              canPool && (
                                <Button
                                  size="sm"
                                  variant="ghost"
                                  loading={busyId === c.id}
                                  onClick={(e) => handleRelease(e, c.id)}
                                >
                                  {busyId === c.id ? '释放中…' : '释放回公海'}
                                </Button>
                              )
                            ) : null}
                          </div>
                        </div>
                      ))
                    )}
                  </div>
                </div>
              ))}
            </div>
          </>
        )}
      </Page>
    </div>
  )
}
