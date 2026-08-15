import api from './client'

// ============ auth ============
export const login = (username, password) =>
  api.post('/auth/login', { username, password }).then((r) => r.data)

export const me = () => api.get('/auth/me').then((r) => r.data)

// ============ users（admin 管理成员） ============
export const createUser = (username, password, role, display_name) =>
  api.post('/users', { username, password, role, display_name }).then((r) => r.data)

export const listUsers = () => api.get('/users').then((r) => r.data)

// ============ customers ============
export const listCustomers = (params) =>
  api.get('/customers', { params }).then((r) => r.data)

export const getCustomer = (id) => api.get(`/customers/${id}`).then((r) => r.data)

export const createCustomer = (data) =>
  api.post('/customers', data).then((r) => r.data)

export const updateCustomer = (id, data) =>
  api.put(`/customers/${id}`, data).then((r) => r.data)

export const deleteCustomer = (id) =>
  api.delete(`/customers/${id}`).then((r) => r.data)

export const checkDuplicate = (data) =>
  api.post('/customers/check-duplicate', data).then((r) => r.data)

export const claimCustomer = (id) =>
  api.post(`/customers/${id}/claim`).then((r) => r.data)

export const releaseCustomer = (id) =>
  api.post(`/customers/${id}/release`).then((r) => r.data)

// ============ activities（时间轴） ============
export const listActivities = (customerId) =>
  api.get(`/customers/${customerId}/activities`).then((r) => r.data)

export const createActivity = (customerId, data) =>
  api.post(`/customers/${customerId}/activities`, data).then((r) => r.data)

// ============ tasks（待办） ============
export const listTasks = () => api.get('/tasks').then((r) => r.data)

export const listCustomerTasks = (customerId) =>
  api.get(`/customers/${customerId}/tasks`).then((r) => r.data)

export const createTask = (customerId, data) =>
  api.post(`/customers/${customerId}/tasks`, data).then((r) => r.data)

export const updateTask = (id, data) =>
  api.put(`/tasks/${id}`, data).then((r) => r.data)

export const deleteTask = (id) =>
  api.delete(`/tasks/${id}`).then((r) => r.data)

// ============ stages（销售阶段） ============
export const listStages = () => api.get('/stages').then((r) => r.data)

export const createStage = (data) =>
  api.post('/stages', data).then((r) => r.data)

export const updateStage = (id, data) =>
  api.put(`/stages/${id}`, data).then((r) => r.data)

export const deleteStage = (id) =>
  api.delete(`/stages/${id}`).then((r) => r.data)

// ============ pipeline（看板/漏斗） ============
export const listPipeline = () => api.get('/pipeline').then((r) => r.data)

export const createPipelineEntry = (data) =>
  api.post('/pipeline', data).then((r) => r.data)

export const updatePipelineEntry = (id, data) =>
  api.put(`/pipeline/${id}`, data).then((r) => r.data)

export const deletePipelineEntry = (id) =>
  api.delete(`/pipeline/${id}`).then((r) => r.data)

export const getKanban = () => api.get('/pipeline/kanban').then((r) => r.data)

export const getFunnel = () => api.get('/pipeline/funnel').then((r) => r.data)

export const getDashboard = () => api.get('/dashboard').then((r) => r.data)

// ============ import/export ============
export const getImportTemplate = () =>
  api.get('/import/template', { responseType: 'blob' }).then((r) => r.data)

export const uploadImport = (csvData) =>
  api.post('/import', csvData, { headers: { 'Content-Type': 'text/csv' } }).then((r) => r.data)

export const listImportBatches = () => api.get('/import/batches').then((r) => r.data)

export const getImportBatch = (id) => api.get(`/import/batches/${id}`).then((r) => r.data)

export const exportCustomersCsv = () =>
  api.get('/export/customers.csv', { responseType: 'blob' }).then((r) => r.data)

// ============ audit ============
export const listAudit = (limit = 100) =>
  api.get('/audit', { params: { limit } }).then((r) => r.data)
