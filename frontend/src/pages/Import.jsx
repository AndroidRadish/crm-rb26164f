import { useEffect, useState } from 'react'
import * as api from '../api'
import { Page, Sidebar } from '../components/Layout'
import { fmtDateTime, TableSkeleton, EmptyState } from '../components/ui'
import { getErrorMessage } from '../api/client'

const navItems = [
  { to: '/customers', icon: 'customers', label: '客户库', end: false },
  { to: '/kanban', icon: 'kanban', label: '销售看板', end: false },
  { to: '/dashboard', icon: 'dashboard', label: '数据看板', end: false },
  { to: '/tasks', icon: 'tasks', label: '我的待办', end: false },
  { to: '/stages', icon: 'stages', label: '销售阶段', end: false },
  { to: '/import', icon: 'import', label: '数据导入', end: false },
  { to: '/users', icon: 'users', label: '成员管理', end: false },
  { to: '/audit', icon: 'audit', label: '审计日志', end: false },
]

export default function Import() {
  const [batches, setBatches] = useState([])
  const [loading, setLoading] = useState(true)
  const [error, setError] = useState('')
  const [csvText, setCsvText] = useState('')
  const [importing, setImporting] = useState(false)
  const [result, setResult] = useState(null)

  const load = async () => {
    setLoading(true)
    try {
      const data = await api.listImportBatches()
      setBatches(data)
      setError('')
    } catch (err) {
      setError(getErrorMessage(err, '加载导入记录失败'))
    } finally {
      setLoading(false)
    }
  }

  useEffect(() => { load() }, [])

  const downloadTemplate = async () => {
    try {
      const blob = await api.getImportTemplate()
      const url = URL.createObjectURL(blob)
      const a = document.createElement('a')
      a.href = url
      a.download = 'customer_template.csv'
      a.click()
      URL.revokeObjectURL(url)
    } catch (err) {
      setError(getErrorMessage(err, '下载模板失败'))
    }
  }

  const doImport = async () => {
    if (!csvText.trim()) { setError('请粘贴 CSV 数据'); return }
    setImporting(true)
    setError('')
    try {
      const data = await api.uploadImport(csvText)
      setResult(data)
      load()
    } catch (err) {
      setError(getErrorMessage(err, '导入失败'))
    } finally {
      setImporting(false)
    }
  }

  const exportCsv = async () => {
    try {
      const blob = await api.exportCustomersCsv()
      const url = URL.createObjectURL(blob)
      const a = document.createElement('a')
      a.href = url
      a.download = 'customers_export.csv'
      a.click()
      URL.revokeObjectURL(url)
    } catch (err) {
      setError(getErrorMessage(err, '导出失败'))
    }
  }

  return (
    <div className="layout">
      <Sidebar navItems={navItems} />
      <Page>
        <h1 className="page-title">数据导入 / 导出</h1>
        {error && <div className="form-error">{error}</div>}

        <div className="card" style={{ marginBottom: 24 }}>
          <div className="card-title">CSV 批量导入</div>
          <div style={{ marginBottom: 12 }}>
            <button className="btn btn-secondary btn-sm" onClick={downloadTemplate}>下载模板</button>
            <button className="btn btn-secondary btn-sm" onClick={exportCsv} style={{ marginLeft: 8 }}>导出客户 CSV</button>
          </div>
          <div className="form-group">
            <label className="form-label">粘贴 CSV 数据（列：公司名,联系人,电话,邮箱,地址,备注,阶段）</label>
            <textarea
              value={csvText}
              onChange={(e) => setCsvText(e.target.value)}
              rows={6}
              placeholder={'示例：\n公司A,张三,13800138000,zhangsan@example.com,上海,备注,线索\n公司B,李四,13800138001,lisi@example.com,北京,备注,意向'}
            />
          </div>
          <button className="btn btn-primary" onClick={doImport} disabled={importing}>
            {importing ? '导入中…' : '开始导入'}
          </button>

          {result && (
            <div className="import-result" style={{ marginTop: 16 }}>
              <div>成功：{result.success_rows} 行，失败：{result.failed_rows} 行</div>
              {result.errors && result.errors.length > 0 && (
                <div style={{ marginTop: 8, fontSize: 13, color: 'var(--color-destructive)' }}>
                  {result.errors.map((e, i) => (
                    <div key={i}>第 {e.row} 行：{e.error}</div>
                  ))}
                </div>
              )}
            </div>
          )}
        </div>

        <div className="card">
          <div className="card-title">导入记录</div>
          {loading ? (
            <TableSkeleton rows={3} />
          ) : batches.length === 0 ? (
            <EmptyState message="暂无导入记录" />
          ) : (
            <div className="table-wrap">
              <table className="data">
                <thead>
                  <tr>
                    <th>文件名</th>
                    <th>总行数</th>
                    <th>成功</th>
                    <th>失败</th>
                    <th>状态</th>
                    <th>时间</th>
                  </tr>
                </thead>
                <tbody>
                  {batches.map((b) => (
                    <tr key={b.id}>
                      <td>{b.filename}</td>
                      <td>{b.total_rows}</td>
                      <td>{b.success_rows}</td>
                      <td>{b.failed_rows}</td>
                      <td>
                        <span className={`badge ${b.status === 'done' ? 'badge-settled' : b.status === 'failed' ? 'badge-past' : 'badge-pending'}`}>
                          {b.status}
                        </span>
                      </td>
                      <td>{fmtDateTime(b.created_at)}</td>
                    </tr>
                  ))}
                </tbody>
              </table>
            </div>
          )}
        </div>
      </Page>
    </div>
  )
}
