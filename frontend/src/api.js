const BASE = ''

async function request(path, options = {}) {
  const token = localStorage.getItem('token')
  const res = await fetch(`${BASE}${path}`, {
    ...options,
    headers: {
      'Content-Type': 'application/json',
      ...(token ? { Authorization: `Bearer ${token}` } : {}),
      ...options.headers,
    },
  })
  if (res.status === 401) {
    localStorage.removeItem('token')
    window.location.href = '/login'
    throw new Error('Unauthorized')
  }
  if (!res.ok) {
    const text = await res.text()
    throw new Error(text || `HTTP ${res.status}`)
  }
  return res.json()
}

export const api = {
  // Auth
  login: (email, password) =>
    request('/api/auth/login', {
      method: 'POST',
      body: JSON.stringify({ email, password }),
    }),

  // Emails
  getEmails: (mailbox = 'INBOX', page = 1) =>
    request(`/api/emails?mailbox=${mailbox}&page=${page}`),
  searchEmails: (query, page = 1) =>
    request(`/api/emails?search=${encodeURIComponent(query)}&page=${page}`),
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
  changePassword: (old_password, new_password) =>
    request('/api/settings/password', {
      method: 'POST',
      body: JSON.stringify({ old_password, new_password }),
    }),
}
