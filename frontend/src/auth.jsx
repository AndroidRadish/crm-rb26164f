import { createContext, useContext, useEffect, useState } from 'react'
import * as api from './api'

const AuthContext = createContext(null)

export function AuthProvider({ children }) {
  const [user, setUser] = useState(() => {
    try {
      return JSON.parse(localStorage.getItem('crm_user'))
    } catch {
      return null
    }
  })
  const [token, setToken] = useState(() => localStorage.getItem('crm_token'))
  // 当前租户授权清单（功能分级）：null = 未加载/拉取失败
  const [features, setFeatures] = useState(null)

  const isAdmin = user?.role === 'admin'

  const loadFeatures = async () => {
    try {
      const list = await api.getEntitlements()
      setFeatures(list)
    } catch {
      // 拉取失败不阻断使用：入口默认放行，后端 403 是底线（spec：两层都要）
      setFeatures(null)
    }
  }

  // 已登录会话刷新页面后补拉授权清单
  useEffect(() => {
    if (token) loadFeatures()
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [])

  const doLogin = async (username, password) => {
    const data = await api.login(username, password)
    localStorage.setItem('crm_token', data.token)
    localStorage.setItem('crm_user', JSON.stringify(data.user))
    setToken(data.token)
    setUser(data.user)
    loadFeatures()
    return data.user
  }

  const logout = () => {
    localStorage.removeItem('crm_token')
    localStorage.removeItem('crm_user')
    setToken(null)
    setUser(null)
    setFeatures(null)
  }

  // 未加载/键不存在时默认放行（体验层）；无授权的真正拦截在后端 403
  const hasFeature = (key) => {
    if (!features) return true
    const f = features.find((x) => x.key === key)
    return f ? !!f.enabled : true
  }

  return (
    <AuthContext.Provider value={{ user, token, isAdmin, doLogin, logout, hasFeature, reloadFeatures: loadFeatures }}>
      {children}
    </AuthContext.Provider>
  )
}

export const useAuth = () => useContext(AuthContext)
