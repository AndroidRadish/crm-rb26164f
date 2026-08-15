import { createContext, useContext, useState } from 'react'
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

  const isAdmin = user?.role === 'admin'

  const doLogin = async (username, password) => {
    const data = await api.login(username, password)
    localStorage.setItem('crm_token', data.token)
    localStorage.setItem('crm_user', JSON.stringify(data.user))
    setToken(data.token)
    setUser(data.user)
    return data.user
  }

  const logout = () => {
    localStorage.removeItem('crm_token')
    localStorage.removeItem('crm_user')
    setToken(null)
    setUser(null)
  }

  return (
    <AuthContext.Provider value={{ user, token, isAdmin, doLogin, logout }}>
      {children}
    </AuthContext.Provider>
  )
}

export const useAuth = () => useContext(AuthContext)
