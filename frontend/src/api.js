const BASE = ''

async function request(path, options = {}) {
  const { authRedirect = true, headers, ...fetchOptions } = options
  const token = localStorage.getItem('token')
  const res = await fetch(`${BASE}${path}`, {
    ...fetchOptions,
    headers: {
      'Content-Type': 'application/json',
      ...(token ? { Authorization: `Bearer ${token}` } : {}),
      ...headers,
    },
  })
  if (res.status === 401) {
    localStorage.removeItem('token')
    if (authRedirect) {
      window.location.href = '/login'
    }
    throw new Error('Unauthorized')
  }
  if (!res.ok) {
    const text = await res.text()
    throw new Error(text || `HTTP ${res.status}`)
  }
  const text = await res.text()
  if (!text) return {}
  try {
    return JSON.parse(text)
  } catch {
    throw new Error(`接口 ${path} 返回了非 JSON 内容，请确认后端 API 已启动并且路由已生效`)
  }
}

function withQuery(path, params) {
  const query = new URLSearchParams()
  Object.entries(params).forEach(([key, value]) => {
    if (value !== undefined && value !== null && value !== '') {
      query.set(key, value)
    }
  })
  const search = query.toString()
  return search ? `${path}?${search}` : path
}

export const api = {
  // Auth
  login: (email, password) =>
    request('/api/auth/login', {
      method: 'POST',
      authRedirect: false,
      body: JSON.stringify({ email, password }),
    }),

  // Setup
  getSetupStatus: () => request('/api/setup/status', { authRedirect: false }),
  runSetup: (data) =>
    request('/api/setup', {
      method: 'POST',
      authRedirect: false,
      body: JSON.stringify(data),
    }),

  // Emails
  getEmails: (mailbox = 'INBOX', page = 1, perPage = 50) =>
    request(withQuery('/api/emails', { mailbox, page, per_page: perPage })),
  searchEmails: (query, page = 1, perPage = 50) =>
    request(withQuery('/api/emails', { search: query, page, per_page: perPage })),
  getEmail: (id) => request(`/api/emails/${id}`),
  deleteEmail: (id) => request(`/api/emails/${id}`, { method: 'DELETE' }),
  markRead: (id) => request(`/api/emails/${id}/read`, { method: 'PUT' }),
  moveEmail: (id, mailbox) =>
    request(`/api/emails/${id}/move`, {
      method: 'PUT',
      body: JSON.stringify({ mailbox }),
    }),
  sendEmail: (data) =>
    request('/api/emails/send', {
      method: 'POST',
      body: JSON.stringify(data),
    }),
  getMailboxCounts: () => request('/api/emails/mailboxes'),

  // Attachments
  getAttachmentUrl: (id) => `/api/attachments/${id}`,

  // Domains
  getDomains: () => request('/api/domains'),
  createDomain: (domain_name) =>
    request('/api/domains', {
      method: 'POST',
      body: JSON.stringify({ domain_name }),
    }),
  deleteDomain: (id) => request(`/api/domains/${id}`, { method: 'DELETE' }),
  generateDkim: (id) => request(`/api/domains/${id}/dkim`, { method: 'POST' }),

  // Users
  getUsers: () => request('/api/users'),
  createUser: (data) =>
    request('/api/users', {
      method: 'POST',
      body: JSON.stringify(data),
    }),
  deleteUser: (id) => request(`/api/users/${id}`, { method: 'DELETE' }),

  // Settings
  getSettings: () => request('/api/settings'),
  updateSettings: (data) =>
    request('/api/settings', {
      method: 'PUT',
      body: JSON.stringify(data),
    }),
  changePassword: (old_password, new_password) =>
    request('/api/settings/password', {
      method: 'POST',
      body: JSON.stringify({ old_password, new_password }),
    }),
}
