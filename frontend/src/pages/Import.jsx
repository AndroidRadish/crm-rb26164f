import { useEffect, useState } from 'react'
import * as api from '../api'
import { Page, Sidebar, getNavItems } from '../components/Layout'
import {
  Button, Callout, Card, EmptyState, ErrorState, Field, PageHeader, Table, TableSkeleton,
  Tag, Textarea, fmtDateTime,
} from '../components/ui'
import { getErrorMessage } from '../api/client'
import { useAuth } from '../auth'
import { FeatureNotice } from '../components/FeatureGate'

const STATUS_TONE = { done: 'success', failed: 'danger', pending: 'info' }

export default function Import() {
  const { user, hasFeature } = useAuth()
  const [batches, setBatches] = useState([])
  const [loading, setLoading] = useState(true)
  const [loadError, setLoadError] = useState('')
  const [actionError, setActionError] = useState('')
  const [csvText, setCsvText] = useState('')
  const [importing, setImporting] = useState(false)
  const [downloading, setDownloading] = useState(false)
  const [exporting, setExporting] = useState(false)
  const [result, setResult] = useState(null)

  const load = async () => {
    setLoading(true)
    try {
      const data = await api.listImportBatches()
      setBatches(data)
      setLoadError('')
    } catch (err) {
      setLoadError(getErrorMessage(err, '加载导入记录失败'))
    } finally {
      setLoading(false)
    }
  }

  useEffect(() => { load() }, [])

  // 功能分级：直链 /import 且无授权 → 提示页（入口已在导航层隐藏，这里是直链底线）
  if (!hasFeature('import.export')) {
    return (
      <div className="layout">
        <Sidebar navItems={getNavItems(user)} />
        <Page>
          <PageHeader title="数据导入 / 导出" />
          <FeatureNotice />
        </Page>
      </div>
    )
  }

  const downloadTemplate = async () => {
    setActionError('')
    setDownloading(true)
    try {
      const blob = await api.getImportTemplate()
      const url = URL.createObjectURL(blob)
      const a = document.createElement('a')
      a.href = url
      a.download = 'customer_template.csv'
      a.click()
      URL.revokeObjectURL(url)
    } catch (err) {
      setActionError(getErrorMessage(err, '下载模板失败'))
    } finally {
      setDownloading(false)
    }
  }

  const doImport = async () => {
    setActionError('')
    if (!csvText.trim()) { setActionError('请粘贴 CSV 数据'); return }
    setImporting(true)
    try {
      const data = await api.uploadImport(csvText)
      setResult(data)
      load()
    } catch (err) {
      setActionError(getErrorMessage(err, '导入失败'))
    } finally {
      setImporting(false)
    }
  }

  const exportCsv = async () => {
    setActionError('')
    setExporting(true)
    try {
      const blob = await api.exportCustomersCsv()
      const url = URL.createObjectURL(blob)
      const a = document.createElement('a')
      a.href = url
      a.download = 'customers_export.csv'
      a.click()
      URL.revokeObjectURL(url)
    } catch (err) {
      setActionError(getErrorMessage(err, '导出失败'))
    } finally {
      setExporting(false)
    }
  }

  return (
    <div className="layout">
      <Sidebar navItems={getNavItems(user)} />
      <Page>
        <PageHeader
          title="数据导入 / 导出"
          description="CSV 批量导入客户；导入结果逐行回执，失败行不影响成功行"
          actions={(
            <>
              <Button variant="secondary" onClick={downloadTemplate} loading={downloading}>下载模板</Button>
              <Button variant="secondary" onClick={exportCsv} loading={exporting}>导出客户 CSV</Button>
            </>
          )}
        />

        {actionError && <Callout tone="danger">{actionError}</Callout>}

        <Card title="CSV 批量导入" className="u-mb-4">
          <Field
            label="粘贴 CSV 数据"
            hint="列顺序：公司名,联系人,电话,邮箱,地址,备注,阶段"
            htmlFor="import-csv"
          >
            <Textarea
              id="import-csv"
              value={csvText}
              onChange={(e) => setCsvText(e.target.value)}
              rows={6}
              placeholder={'示例：\n公司A,张三,13800138000,zhangsan@example.com,上海,备注,线索\n公司B,李四,13800138001,lisi@example.com,北京,备注,意向'}
            />
          </Field>
          <Button variant="primary" onClick={doImport} loading={importing}>
            {importing ? '导入中…' : '开始导入'}
          </Button>

          {result && (
            <div className="import-result">
              <div>
                成功：{result.success_rows} 行，失败：{result.failed_rows} 行
              </div>
              {result.errors && result.errors.length > 0 && (
                <div className="import-error-list">
                  {result.errors.map((e, i) => (
                    <div key={i} className="import-errors-row">第 {e.row} 行：{e.error}</div>
                  ))}
                </div>
              )}
            </div>
          )}
        </Card>

        <Card title="导入记录">
          {loadError ? (
            <ErrorState title="导入记录加载失败" message={loadError} onRetry={load} />
          ) : loading ? (
            <TableSkeleton rows={3} />
          ) : batches.length === 0 ? (
            <EmptyState
              inline
              title="暂无导入记录"
              description="粘贴 CSV 并导入后，这里会记录每批的成功/失败行数与处理状态。"
              action={<Button size="sm" variant="secondary" onClick={downloadTemplate} loading={downloading}>下载模板</Button>}
            />
          ) : (
            <Table columns={['文件名', '总行数', '成功', '失败', '状态', '时间']}>
              {batches.map((b) => (
                <tr key={b.id}>
                  <td>{b.filename}</td>
                  <td>{b.total_rows}</td>
                  <td>{b.success_rows}</td>
                  <td>{b.failed_rows}</td>
                  <td><Tag tone={STATUS_TONE[b.status] || 'neutral'}>{b.status}</Tag></td>
                  <td className="cell-muted">{fmtDateTime(b.created_at)}</td>
                </tr>
              ))}
            </Table>
          )}
        </Card>
      </Page>
    </div>
  )
}
