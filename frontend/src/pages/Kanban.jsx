import { useEffect, useState } from 'react'
import { useNavigate } from 'react-router-dom'
import * as api from '../api'
import { Page, Sidebar } from '../components/Layout'
import { TableSkeleton } from '../components/ui'
import { getErrorMessage } from '../api/client'
import { useAuth } from '../auth'

const navItems = [
  { to: '/customers', icon: 'customers', label: '客户库', end: false },
  { to: '/kanban', icon: 'kanban', label: '销售看板', end: false },
  { to: '/dashboard', icon: 'dashboard', label: '数据看板', end: false },
  { to: '/tasks', icon: 'tasks', label: '我的待办', end: false },
]

export default function Kanban() {
  const navigate = useNavigate()
  const { user } = useAuth()
  const [columns, setColumns] = useState([])
  const [loading, setLoading] = useState(true)
  const [error, setError] = useState('')
  const [dragStageId, setDragStageId] = useState(null)
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
    setDragStageId(stageId)
  }

  const handleDrop = (e, targetStageId) => {
    e.preventDefault()
    const customerId = e.dataTransfer.getData('text/plain')
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

  const handleDragOver = (e) => {
    e.preventDefault()
  }

  if (loading) return <div className="layout"><Page><TableSkeleton rows={4} /></Page></div>

  return (
    <div className="layout">
      <Sidebar navItems={navItems} />
      <Page>
        <h1 className="page-title">销售看板</h1>
        {error && <div className="form-error">{error}</div>}
        <div className="kanban-board">
          {columns.map((col) => (
            <div
              key={col.stage_id}
              className="kanban-column"
              onDragOver={handleDragOver}
              onDrop={(e) => handleDrop(e, col.stage_id)}
            >
              <div className="kanban-column-header">
                <span className="kanban-column-name">{col.stage_name}</span>
                <span className="kanban-column-count">{col.customers.length}</span>
              </div>
              <div className="kanban-column-body">
                {col.customers.length === 0 ? (
                  <div className="kanban-empty">拖拽客户到此</div>
                ) : (
                  col.customers.map((c) => (
                    <div
                      key={c.id}
                      className="kanban-card"
                      draggable={!busyId}
                      onDragStart={(e) => handleDragStart(e, c.id, col.stage_id)}
                      onClick={() => navigate(`/customers/${c.id}`)}
                    >
                      <div className="kanban-card-company">{c.company}</div>
                      <div className="kanban-card-contact">
                        {c.contact_name || '—'} {c.phone ? `· ${c.phone}` : ''}
                      </div>
                      <div className="kanban-card-owner">
                        {c.owner_id ? (
                          isMine(c.owner_id) ? (
                            <span className="kanban-owner-mine">负责人：我</span>
                          ) : (
                            <span className="kanban-owner-other">负责人：{c.owner_name || '他人'}</span>
                          )
                        ) : (
                          <span className="kanban-owner-pool">公海</span>
                        )}
                      </div>
                      <div className="kanban-card-actions">
                        {!c.owner_id ? (
                          <button
                            className="btn btn-sm btn-primary"
                            disabled={busyId === c.id}
                            onClick={(e) => handleClaim(e, c.id)}
                          >
                            {busyId === c.id ? '认领中…' : '加入手头'}
                          </button>
                        ) : isMine(c.owner_id) ? (
                          <button
                            className="btn btn-sm btn-ghost"
                            disabled={busyId === c.id}
                            onClick={(e) => handleRelease(e, c.id)}
                          >
                            {busyId === c.id ? '释放中…' : '释放回公海'}
                          </button>
                        ) : null}
                      </div>
                    </div>
                  ))
                )}
              </div>
            </div>
          ))}
        </div>
      </Page>
    </div>
  )
}
