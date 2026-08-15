import { useEffect, useState } from 'react'
import { useNavigate } from 'react-router-dom'
import * as api from '../api'
import { Page, Sidebar } from '../components/Layout'
import { TableSkeleton } from '../components/ui'
import { getErrorMessage } from '../api/client'

const navItems = [
  { to: '/customers', icon: 'customers', label: '客户库', end: false },
  { to: '/kanban', icon: 'kanban', label: '销售看板', end: false },
  { to: '/dashboard', icon: 'dashboard', label: '数据看板', end: false },
  { to: '/tasks', icon: 'tasks', label: '我的待办', end: false },
]

export default function Kanban() {
  const navigate = useNavigate()
  const [columns, setColumns] = useState([])
  const [loading, setLoading] = useState(true)
  const [error, setError] = useState('')
  const [dragStageId, setDragStageId] = useState(null)

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

  const handleDragStart = (e, customerId, stageId) => {
    e.dataTransfer.setData('text/plain', customerId)
    setDragStageId(stageId)
  }

  const handleDrop = async (e, targetStageId) => {
    e.preventDefault()
    const customerId = e.dataTransfer.getData('text/plain')
    if (!customerId || customerId === dragStageId) return

    try {
      await api.updateCustomer(customerId, { stage: targetStageId })
      load()
    } catch (err) {
      setError(getErrorMessage(err, '移动客户失败'))
    }
    setDragStageId(null)
  }

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
                      draggable
                      onDragStart={(e) => handleDragStart(e, c.id, col.stage_id)}
                      onClick={() => navigate(`/customers/${c.id}`)}
                    >
                      <div className="kanban-card-company">{c.company}</div>
                      <div className="kanban-card-contact">
                        {c.contact_name || '—'} {c.phone ? `· ${c.phone}` : ''}
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
