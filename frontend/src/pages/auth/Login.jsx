import { useState } from 'react'
import { useNavigate } from 'react-router-dom'
import { useAuth } from '../../auth'
import { getErrorMessage } from '../../api/client'
import './auth.css'

export default function Login() {
  const { doLogin } = useAuth()
  const navigate = useNavigate()
  const [username, setUsername] = useState('')
  const [password, setPassword] = useState('')
  const [error, setError] = useState('')
  const [loading, setLoading] = useState(false)

  const submit = async (e) => {
    e.preventDefault()
    setError('')
    setLoading(true)
    try {
      const user = await doLogin(username.trim(), password)
      navigate('/customers', { replace: true })
    } catch (err) {
      setError(getErrorMessage(err, '登录失败'))
    } finally {
      setLoading(false)
    }
  }

  return (
    <div className="auth-page">
      <div className="auth-card">
        <div className="auth-brand"><span className="dot">●</span> CRM</div>
        <h1 className="auth-title">登录</h1>
        <p className="auth-sub">轻量级客户关系管理</p>
        {error && <div className="form-error">{error}</div>}
        <form onSubmit={submit}>
          <div className="form-group">
            <label className="form-label">用户名</label>
            <input value={username} onChange={(e) => setUsername(e.target.value)} autoFocus />
          </div>
          <div className="form-group">
            <label className="form-label">密码</label>
            <input type="password" value={password} onChange={(e) => setPassword(e.target.value)} />
          </div>
          <button type="submit" className="btn btn-primary" style={{ width: '100%', marginTop: 8 }} disabled={loading}>
            {loading ? '登录中…' : '登录'}
          </button>
        </form>
      </div>
    </div>
  )
}
