export const mailboxCountsChangedEvent = 'kuria:mailbox-counts-changed'

export function notifyMailboxCountsChanged() {
  window.dispatchEvent(new CustomEvent(mailboxCountsChangedEvent))
}
