import { useState } from 'react'
import { NavLink, useNavigate } from 'react-router-dom'
import { useAuth } from '../auth'

const icons = {
  logo: (
    <svg fill="none" stroke="currentColor" strokeWidth="1.8" viewBox="0 0 24 24" aria-hidden="true"><path strokeLinecap="round" strokeLinejoin="round" d="M12 21c4.5-2.4 7-6 7-10a5 5 0 00-5-5c-.7 0-1.4.1-2 .4A5 5 0 007 6a5 5 0 00-5 5c0 4 2.5 7.6 7 10a4 4 0 002 0z" /><path strokeLinecap="round" strokeLinejoin="round" d="M12 12v9" /></svg>
  ),
  customers: (
    <svg fill="none" stroke="currentColor" strokeWidth="1.8" viewBox="0 0 24 24" aria-hidden="true"><path strokeLinecap="round" strokeLinejoin="round" d="M17 20h5v-2a3 3 0 00-5.356-1.857M17 20H7m10 0v-2c0-.656-.126-1.283-.356-1.857M7 20H2v-2a3 3 0 015.356-1.857M7 20v-2c0-.656.126-1.283.356-1.857m0 0a5.002 5.002 0 019.288 0M15 7a3 3 0 11-6 0 3 3 0 016 0zm6 3a2 2 0 11-4 0 2 2 0 014 0zM7 10a2 2 0 11-4 0 2 2 0 014 0z" /></svg>
  ),
  kanban: (
    <svg fill="none" stroke="currentColor" strokeWidth="1.8" viewBox="0 0 24 24" aria-hidden="true"><path strokeLinecap="round" strokeLinejoin="round" d="M4 5h4v14H4zM10 5h4v9h-4zM16 5h4v6h-4z" /></svg>
  ),
  dashboard: (
    <svg fill="none" stroke="currentColor" strokeWidth="1.8" viewBox="0 0 24 24" aria-hidden="true"><path strokeLinecap="round" strokeLinejoin="round" d="M3 13h8V3H3v10zm0 8h8v-6H3v6zm10 0h8V11h-8v10zm0-18v6h8V3h-8z" /></svg>
  ),
  tasks: (
    <svg fill="none" stroke="currentColor" strokeWidth="1.8" viewBox="0 0 24 24" aria-hidden="true"><path strokeLinecap="round" strokeLinejoin="round" d="M9 5H7a2 2 0 00-2 2v12a2 2 0 002 2h10a2 2 0 002-2V7a2 2 0 00-2-2h-2M9 5a2 2 0 002 2h2a2 2 0 002-2M9 5a2 2 0 012-2h2a2 2 0 012 2m-6 9l2 2 4-4" /></svg>
  ),
  stages: (
    <svg fill="none" stroke="currentColor" strokeWidth="1.8" viewBox="0 0 24 24" aria-hidden="true"><path strokeLinecap="round" strokeLinejoin="round" d="M4 7h16M4 12h12M4 17h8" /></svg>
  ),
  import: (
    <svg fill="none" stroke="currentColor" strokeWidth="1.8" viewBox="0 0 24 24" aria-hidden="true"><path strokeLinecap="round" strokeLinejoin="round" d="M4 16v1a3 3 0 003 3h10a3 3 0 003-3v-1m-4-8l-4-4m0 0L8 8m4-4v12" /></svg>
  ),
  users: (
    <svg fill="none" stroke="currentColor" strokeWidth="1.8" viewBox="0 0 24 24" aria-hidden="true"><path strokeLinecap="round" strokeLinejoin="round" d="M17 20h5v-2a3 3 0 00-5.356-1.857M17 20H7m10 0v-2c0-.656-.126-1.283-.356-1.857M7 20H2v-2a3 3 0 015.356-1.857M7 20v-2c0-.656.126-1.283.356-1.857m0 0a5.002 5.002 0 019.288 0M15 7a3 3 0 11-6 0 3 3 0 016 0z" /></svg>
  ),
  audit: (
    <svg fill="none" stroke="currentColor" strokeWidth="1.8" viewBox="0 0 24 24" aria-hidden="true"><path strokeLinecap="round" strokeLinejoin="round" d="M9 12h6m-6 4h6m2 5H7a2 2 0 01-2-2V5a2 2 0 012-2h5.586a1 1 0 01.707.293l5.414 5.414a1 1 0 01.293.707V19a2 2 0 01-2 2z" /></svg>
  ),
  logout: (
    <svg fill="none" stroke="currentColor" strokeWidth="1.8" viewBox="0 0 24 24" aria-hidden="true"><path strokeLinecap="round" strokeLinejoin="round" d="M17 16l4-4m0 0l-4-4m4 4H7m6 4v1a3 3 0 01-3 3H6a3 3 0 01-3-3V7a3 3 0 013-3h4a3 3 0 013 3v1" /></svg>
  ),
  menu: (
    <svg fill="none" stroke="currentColor" strokeWidth="2" viewBox="0 0 24 24" aria-hidden="true"><path strokeLinecap="round" strokeLinejoin="round" d="M4 6h16M4 12h16M4 18h16" /></svg>
  ),
  reports: (
    <svg fill="none" stroke="currentColor" strokeWidth="1.8" viewBox="0 0 24 24" aria-hidden="true"><path strokeLinecap="round" strokeLinejoin="round" d="M9 17v-6m4 6V7m4 10v-3M5 21h14a2 2 0 002-2V5a2 2 0 00-2-2H5a2 2 0 00-2 2v14a2 2 0 002 2z" /></svg>
  ),
}

// 统一导航（RB-266FB3）：带 feature 标记的项在无授权时由 Sidebar 隐藏；
// admin 专属项仅老板角色可见。各页面不再各自维护重复清单。
// 注意：本函数是权限可见性的唯一入口，本单（纯视觉）不做任何改动。
export function getNavItems(user) {
  const items = [
    { to: '/customers', icon: 'customers', label: '客户库', end: false, feature: 'crm.customers' },
    { to: '/kanban', icon: 'kanban', label: '销售看板', end: false, feature: 'crm.kanban' },
    { to: '/dashboard', icon: 'dashboard', label: '数据看板', end: false },
    { to: '/tasks', icon: 'tasks', label: '我的待办', end: false, feature: 'crm.tasks' },
    { to: '/reports', icon: 'reports', label: '报表', end: false, feature: 'crm.reports' },
  ]
  if (user?.role === 'admin') {
    items.push(
      { to: '/stages', icon: 'stages', label: '销售阶段', end: false },
      { to: '/import', icon: 'import', label: '数据导入', end: false, feature: 'import.export' },
      { to: '/users', icon: 'users', label: '成员管理', end: false },
      { to: '/audit', icon: 'audit', label: '审计日志', end: false },
    )
  }
  return items
}

let setSidebarOpenGlobal = null

export function Sidebar({ navItems }) {
  const { logout, hasFeature } = useAuth()
  const navigate = useNavigate()
  const [open, setOpen] = useState(false)
  setSidebarOpenGlobal = setOpen

  const close = () => setOpen(false)
  // 功能分级：带 feature 标记的导航项在无授权时隐藏入口
  const visibleItems = navItems.filter((item) => !item.feature || hasFeature(item.feature))

  return (
    <>
      <div className={`sidebar-overlay${open ? ' show' : ''}`} onClick={close} />
      <aside className={`sidebar${open ? ' open' : ''}`}>
        <div className="sidebar-brand">
          <span className="brand-mark">{icons.logo}</span>
          <span className="brand-text">
            CRM
            <span className="brand-sub">Radish</span>
          </span>
        </div>
        <nav className="sidebar-nav">
          {visibleItems.map((item) => (
            <NavLink
              key={item.to}
              to={item.to}
              end={item.end}
              className={({ isActive }) => `sidebar-link${isActive ? ' active' : ''}`}
              onClick={close}
            >
              {icons[item.icon]}
              {item.label}
            </NavLink>
          ))}
        </nav>
        <div className="sidebar-footer">
          <button
            className="sidebar-link"
            onClick={() => { logout(); navigate('/auth/login') }}
          >
            {icons.logout}
            退出登录
          </button>
        </div>
      </aside>
    </>
  )
}

export function Topbar() {
  const { user } = useAuth()
  const name = user?.display_name || user?.username || '用户'
  const initial = [...name][0] || '用'
  return (
    <div className="topbar">
      <div className="user-chip">
        <span className="user-avatar">{initial}</span>
        <span>{name}</span>
        <span className={`role-tag${user?.role === 'admin' ? ' is-admin' : ''}`}>
          {user?.role === 'admin' ? 'ADMIN' : 'MEMBER'}
        </span>
      </div>
    </div>
  )
}

export function Page({ children }) {
  const openSidebar = () => setSidebarOpenGlobal?.(true)
  return (
    <div className="main">
      <button className="hamburger" onClick={openSidebar}>
        {icons.menu}
        菜单
      </button>
      <Topbar />
      {children}
    </div>
  )
}

export { icons }
