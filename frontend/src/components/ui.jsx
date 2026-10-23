import { useEffect } from 'react'
import { createPortal } from 'react-dom'

/* ==========================================================================
   共享 UI 组件（全部样式来自 styles/components.css，取值来自 tokens.css）
   页面只引用这里的组件，不再各自造按钮 / 输入 / 卡片 / 表格 / 标签 / 弹窗。
   ========================================================================== */

export function cx(...parts) {
  return parts.filter(Boolean).join(' ')
}

export function fmtDate(iso) {
  if (!iso) return '—'
  return new Date(iso).toLocaleDateString('zh-CN', { year: 'numeric', month: '2-digit', day: '2-digit' })
}

export function fmtDateTime(iso) {
  if (!iso) return '—'
  return new Date(iso).toLocaleString('zh-CN', {
    month: '2-digit', day: '2-digit', hour: '2-digit', minute: '2-digit',
  })
}

/* ------------------------------ 按钮 ------------------------------ */

export function Spinner({ size = 'sm' }) {
  return <span className={cx('spinner', size === 'lg' && 'spinner-lg')} aria-hidden="true" />
}

export function Button({
  variant = 'secondary',
  size = 'md',
  loading = false,
  block = false,
  className = '',
  children,
  disabled,
  type = 'button',
  ...rest
}) {
  return (
    <button
      type={type}
      className={cx(
        'btn',
        `btn-${variant}`,
        size === 'sm' && 'btn-sm',
        size === 'lg' && 'btn-lg',
        block && 'btn-block',
        className,
      )}
      disabled={disabled || loading}
      {...rest}
    >
      {loading && <Spinner />}
      {children}
    </button>
  )
}

/* ------------------------------ 表单 ------------------------------ */

export function Input({ className = '', ...rest }) {
  return <input className={cx('input', className)} {...rest} />
}

export function Select({ className = '', children, ...rest }) {
  return (
    <select className={cx('select', className)} {...rest}>
      {children}
    </select>
  )
}

export function Textarea({ className = '', ...rest }) {
  return <textarea className={cx('textarea', className)} {...rest} />
}

export function Checkbox({ label, className = '', ...rest }) {
  return (
    <label className={cx('checkbox', className)}>
      <input type="checkbox" {...rest} />
      <span className="checkbox-label">{label}</span>
    </label>
  )
}

export function Field({ label, htmlFor, required = false, hint, error, className = '', children }) {
  return (
    <div className={cx('field', className)}>
      {label && (
        <label className="field-label" htmlFor={htmlFor}>
          {label}
          {required && <span className="field-required">*</span>}
        </label>
      )}
      {children}
      {error ? (
        <span className="field-error">{error}</span>
      ) : hint ? (
        <span className="field-hint">{hint}</span>
      ) : null}
    </div>
  )
}

/* ------------------------------ 卡片 ------------------------------ */

export function Card({ title, subtitle, actions, footer, children, className = '', bodyClassName = '', ...rest }) {
  const hasHeader = Boolean(title || subtitle || actions)
  return (
    <section className={cx('card', className)} {...rest}>
      {hasHeader && (
        <header className="card-header">
          <div className="card-heading">
            {title && <div className="card-title">{title}</div>}
            {subtitle && <div className="card-subtitle">{subtitle}</div>}
          </div>
          {actions && <div className="card-actions">{actions}</div>}
        </header>
      )}
      {children != null && <div className={cx('card-body', bodyClassName)}>{children}</div>}
      {footer && <footer className="card-footer">{footer}</footer>}
    </section>
  )
}

/* ------------------------------ 表格 ------------------------------ */

export function Table({ columns, children, className = '', ...rest }) {
  return (
    <div className={cx('table-wrap', className)}>
      <div className="table-scroll">
        <table className="data" {...rest}>
          <thead>
            <tr>
              {columns.map((col, i) => {
                const c = typeof col === 'string' ? { label: col } : col
                return (
                  <th key={c.key || c.label || i} style={c.width ? { width: c.width } : undefined}>
                    {c.label}
                  </th>
                )
              })}
            </tr>
          </thead>
          <tbody>{children}</tbody>
        </table>
      </div>
    </div>
  )
}

/* ------------------------------ 标签 ------------------------------ */

const TAG_TONES = {
  accent: 'tag-accent',
  success: 'tag-success',
  warning: 'tag-warning',
  danger: 'tag-danger',
  info: 'tag-info',
  neutral: 'tag-neutral',
  muted: 'tag-muted',
}

export function Tag({ tone = 'neutral', dot = true, className = '', children }) {
  return (
    <span className={cx('tag', TAG_TONES[tone] || TAG_TONES.neutral, className)}>
      {dot && <span className="tag-dot" />}
      {children}
    </span>
  )
}

/* ------------------------------ 弹窗 ------------------------------ */

export function Modal({
  open,
  title,
  description,
  onClose,
  footer,
  children,
  size = 'md',
  className = '',
}) {
  useEffect(() => {
    if (!open) return undefined
    const onKeyDown = (e) => {
      if (e.key === 'Escape') onClose?.()
    }
    window.addEventListener('keydown', onKeyDown)
    const previousOverflow = document.body.style.overflow
    document.body.style.overflow = 'hidden'
    return () => {
      window.removeEventListener('keydown', onKeyDown)
      document.body.style.overflow = previousOverflow
    }
  }, [open, onClose])

  if (!open) return null

  return createPortal(
    <div
      className="modal-overlay"
      onMouseDown={(e) => {
        if (e.target === e.currentTarget) onClose?.()
      }}
    >
      <div
        className={cx('modal', size === 'sm' && 'modal--sm', size === 'lg' && 'modal--lg', className)}
        role="dialog"
        aria-modal="true"
        aria-label={title}
      >
        <div className="modal-header">
          <div className="card-heading">
            <div className="modal-title">{title}</div>
            {description && <div className="modal-desc">{description}</div>}
          </div>
          {onClose && (
            <button type="button" className="modal-close" onClick={onClose} aria-label="关闭">
              ×
            </button>
          )}
        </div>
        <div className="modal-body">{children}</div>
        {footer && <div className="modal-footer">{footer}</div>}
      </div>
    </div>,
    document.body,
  )
}

/* ------------------------------ 空态 / 错误态 / 加载态 ------------------------------ */

const INBOX_ICON = (
  <svg fill="none" stroke="currentColor" strokeWidth="1.6" viewBox="0 0 24 24" aria-hidden="true">
    <path strokeLinecap="round" strokeLinejoin="round" d="M3 13h4l2 3h6l2-3h4M5 5h14a2 2 0 012 2v10a2 2 0 01-2 2H5a2 2 0 01-2-2V7a2 2 0 012-2z" />
  </svg>
)

export function EmptyState({ title = '暂无数据', description, action, icon, inline = false, className = '' }) {
  return (
    <div className={cx('empty-state', inline && 'empty-state--inline', className)}>
      <div className="empty-icon" aria-hidden="true">{icon || INBOX_ICON}</div>
      <div className="empty-title">{title}</div>
      {description && <div className="empty-desc">{description}</div>}
      {action && <div className="empty-action">{action}</div>}
    </div>
  )
}

export function ErrorState({ title = '加载失败', message, onRetry, retryLabel = '重试', className = '' }) {
  return (
    <div className={cx('error-state', className)} role="alert">
      <div className="error-icon" aria-hidden="true">!</div>
      <div className="error-title">{title}</div>
      {message && <div className="error-desc">{message}</div>}
      {onRetry && (
        <div className="error-action">
          <Button variant="secondary" onClick={onRetry}>{retryLabel}</Button>
        </div>
      )}
    </div>
  )
}

export function LoadingState({ label = '加载中…' }) {
  return (
    <div className="loading-state" role="status">
      <Spinner />
      <span>{label}</span>
    </div>
  )
}

/* ------------------------------ 页头 / 指标 / 提示 ------------------------------ */

export function PageHeader({ title, description, actions }) {
  return (
    <div className="page-header">
      <div className="page-heading">
        <h1 className="page-title">{title}</h1>
        {description && <div className="page-subtitle">{description}</div>}
      </div>
      {actions && <div className="page-actions">{actions}</div>}
    </div>
  )
}

const STAT_TONES = {
  accent: 'stat-value--accent',
  danger: 'stat-value--danger',
  warning: 'stat-value--warning',
  default: '',
}

export function StatCard({ label, value, hint, tone = 'default' }) {
  return (
    <div className="stat-card">
      <div className="stat-label">{label}</div>
      <div className={cx('stat-value', STAT_TONES[tone] || '')}>{value}</div>
      {hint && <div className="stat-hint">{hint}</div>}
    </div>
  )
}

const CALLOUT_TONES = {
  danger: 'callout-danger',
  warning: 'callout-warning',
  success: 'callout-success',
  info: 'callout-info',
}

const CALLOUT_ICONS = {
  danger: '!',
  warning: '!',
  success: '✓',
  info: 'i',
}

export function Callout({ tone = 'info', title, children, actions, className = '' }) {
  return (
    <div className={cx('callout', CALLOUT_TONES[tone] || CALLOUT_TONES.info, className)} role={tone === 'danger' ? 'alert' : undefined}>
      <span className="callout-icon" aria-hidden="true">{CALLOUT_ICONS[tone] || CALLOUT_ICONS.info}</span>
      <div className="callout-content">
        {title && <div className="callout-title">{title}</div>}
        {children}
        {actions && <div className="callout-actions">{actions}</div>}
      </div>
    </div>
  )
}

/* ------------------------------ 骨架屏 ------------------------------ */

export function Skeleton({ width, height, className = '', style }) {
  return (
    <div
      className={cx('skeleton', className)}
      style={{ width, height, ...style }}
      aria-hidden="true"
    />
  )
}

export function TableSkeleton({ rows = 6 }) {
  return (
    <div className="table-wrap">
      <div className="skeleton-table">
        {Array.from({ length: rows }).map((_, i) => (
          <Skeleton
            key={i}
            className="skeleton-line"
            style={{ width: `${100 - (i % 4) * 9}%` }}
          />
        ))}
      </div>
    </div>
  )
}

export function CardSkeleton({ count = 4 }) {
  return (
    <div className="stat-grid">
      {Array.from({ length: count }).map((_, i) => (
        <div key={i} className="stat-card">
          <Skeleton width="40%" height={12} />
          <Skeleton width="58%" height={30} />
          <Skeleton width="72%" height={10} />
        </div>
      ))}
    </div>
  )
}

export function ListSkeleton({ rows = 4, className = '' }) {
  return (
    <div className={className}>
      {Array.from({ length: rows }).map((_, i) => (
        <div key={i} className="task-item">
          <div className="u-flex-1">
            <Skeleton width={`${58 - (i % 3) * 8}%`} height={13} />
            <Skeleton width="34%" height={10} className="u-mt-2" />
          </div>
          <Skeleton width={64} height={28} />
        </div>
      ))}
    </div>
  )
}

export function BoardSkeleton({ columns = 4 }) {
  return (
    <div className="kanban-board">
      {Array.from({ length: columns }).map((_, c) => (
        <div key={c} className="kanban-column">
          <div className="kanban-column-header">
            <Skeleton width="55%" height={12} />
            <Skeleton width={20} height={20} />
          </div>
          <div className="kanban-column-body">
            {Array.from({ length: 3 }).map((__, i) => (
              <Skeleton key={i} height={78} />
            ))}
          </div>
        </div>
      ))}
    </div>
  )
}
