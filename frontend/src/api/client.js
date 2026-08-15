import axios from 'axios'

const api = axios.create({
  baseURL: import.meta.env.VITE_API_BASE || '/api',
  headers: { 'Content-Type': 'application/json' },
})

api.interceptors.request.use((config) => {
  const token = localStorage.getItem('crm_token')
  if (token) config.headers.Authorization = `Bearer ${token}`
  return config
})

api.interceptors.response.use(
  (resp) => resp,
  (err) => {
    if (err.response?.status === 401) {
      localStorage.removeItem('crm_token')
      localStorage.removeItem('crm_user')
      if (!window.location.pathname.includes('/auth/')) {
        window.location.href = import.meta.env.BASE_URL + 'auth/login'
      }
    }
    return Promise.reject(err)
  }
)

export default api

export const getErrorMessage = (err, fallback = '操作失败') => {
  return err?.response?.data?.error || fallback
}
