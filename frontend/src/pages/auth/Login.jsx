import { useState } from 'react'
import { useNavigate } from 'react-router-dom'
import { useAuth } from '../../auth'
import { getErrorMessage } from '../../api/client'
import { Button, Callout, Field, Input } from '../../components/ui'
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

  // 空态：表单尚未输入时的引导（非错误）
  const isBlank = !username && !password

  return (
    <div className="auth-page">
      <div className="auth-card">
        <div className="auth-brand">
          <span className="brand-mark" aria-hidden="true">
            <svg fill="none" stroke="currentColor" strokeWidth="1.8" viewBox="0 0 24 24">
              <path strokeLinecap="round" strokeLinejoin="round" d="M12 21c4.5-2.4 7-6 7-10a5 5 0 00-5-5c-.7 0-1.4.1-2 .4A5 5 0 007 6a5 5 0 00-5 5c0 4 2.5 7.6 7 10a4 4 0 002 0z" />
              <path strokeLinecap="round" strokeLinejoin="round" d="M12 12v9" />
            </svg>
          </span>
          CRM
        </div>
        <h1 className="auth-title">登录</h1>
        <p className="auth-sub">轻量级客户关系管理</p>

        {error && <Callout tone="danger">{error}</Callout>}
        {isBlank && !error && (
          <div className="auth-hint">
            使用管理员分配的账号登录；忘记密码请联系管理员重置。
          </div>
        )}

        <form onSubmit={submit}>
          <Field label="用户名" htmlFor="login-username">
            <Input
              id="login-username"
              value={username}
              onChange={(e) => setUsername(e.target.value)}
              autoComplete="username"
              autoFocus
            />
          </Field>
          <Field label="密码" htmlFor="login-password">
            <Input
              id="login-password"
              type="password"
              value={password}
              onChange={(e) => setPassword(e.target.value)}
              autoComplete="current-password"
            />
          </Field>
          <Button type="submit" variant="primary" block loading={loading} className="u-mt-2">
            {loading ? '登录中…' : '登录'}
          </Button>
        </form>

        <div className="auth-footer">Radish Studio · 客户关系管理</div>
      </div>
    </div>
  )
}
