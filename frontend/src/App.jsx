import { Routes, Route, Navigate } from 'react-router-dom'
import { useAuth } from './auth'
import Login from './pages/auth/Login'
import Customers from './pages/Customers'
import CustomerDetail from './pages/CustomerDetail'
import Kanban from './pages/Kanban'
import Dashboard from './pages/Dashboard'
import Tasks from './pages/Tasks'
import Reports from './pages/Reports'
import Stages from './pages/Stages'
import Import from './pages/Import'
import Users from './pages/Users'
import Audit from './pages/Audit'
import Owner from './pages/Owner'

function RequireAuth({ children }) {
  const { user } = useAuth()
  if (!user) return <Navigate to="/auth/login" replace />
  return children
}

function RequireAdmin({ children }) {
  const { user } = useAuth()
  if (!user) return <Navigate to="/auth/login" replace />
  if (user.role !== 'admin') return <Navigate to="/customers" replace />
  return children
}

function HomeRedirect() {
  const { user } = useAuth()
  if (!user) return <Navigate to="/auth/login" replace />
  return <Navigate to="/customers" replace />
}

export default function App() {
  return (
    <Routes>
      <Route path="/" element={<HomeRedirect />} />
      <Route path="/auth/login" element={<Login />} />

      {/* 工作室管理终端（owner 令牌，不走 CRM 登录） */}
      <Route path="/owner" element={<Owner />} />

      {/* 通用页面（admin + member） */}
      <Route path="/customers" element={<RequireAuth><Customers /></RequireAuth>} />
      <Route path="/customers/:id" element={<RequireAuth><CustomerDetail /></RequireAuth>} />
      <Route path="/kanban" element={<RequireAuth><Kanban /></RequireAuth>} />
      <Route path="/dashboard" element={<RequireAuth><Dashboard /></RequireAuth>} />
      <Route path="/tasks" element={<RequireAuth><Tasks /></RequireAuth>} />
      <Route path="/reports" element={<RequireAuth><Reports /></RequireAuth>} />

      {/* 仅 admin */}
      <Route path="/stages" element={<RequireAdmin><Stages /></RequireAdmin>} />
      <Route path="/import" element={<RequireAdmin><Import /></RequireAdmin>} />
      <Route path="/users" element={<RequireAdmin><Users /></RequireAdmin>} />
      <Route path="/audit" element={<RequireAdmin><Audit /></RequireAdmin>} />

      <Route path="*" element={<Navigate to="/" replace />} />
    </Routes>
  )
}
