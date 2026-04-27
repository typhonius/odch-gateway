import { h, render } from 'preact';
import { useState, useEffect, useRef, useCallback } from 'preact/hooks';
import htm from 'htm';

const html = htm.bind(h);

// ---- API helpers ----

async function api(path, opts = {}) {
  const resp = await fetch(`/api/v1${path}`, {
    credentials: 'same-origin',
    headers: { 'Content-Type': 'application/json', ...opts.headers },
    ...opts,
  });
  if (resp.status === 401) {
    window.location.hash = '#/login';
    throw new Error('Unauthorized');
  }
  const data = await resp.json();
  if (!resp.ok) {
    throw new Error(data?.error?.message || resp.statusText);
  }
  return data;
}

function formatSize(bytes) {
  if (!bytes || bytes === 0) return '0 B';
  const units = ['B', 'KiB', 'MiB', 'GiB', 'TiB'];
  const i = Math.floor(Math.log(bytes) / Math.log(1024));
  return (bytes / Math.pow(1024, i)).toFixed(1) + ' ' + units[i];
}

function formatTime(ts) {
  if (!ts) return '';
  const d = typeof ts === 'number' ? new Date(ts * 1000) : new Date(ts);
  return d.toLocaleString();
}

// ---- Toast ----

let toastTimeout;
function Toast({ message, type }) {
  if (!message) return null;
  return html`<div class="toast ${type || ''}">${message}</div>`;
}

// ---- Login Page ----

function LoginPage({ onLogin }) {
  const [username, setUsername] = useState('');
  const [password, setPassword] = useState('');
  const [error, setError] = useState('');
  const [loading, setLoading] = useState(false);

  const submit = async (e) => {
    e.preventDefault();
    setLoading(true);
    setError('');
    try {
      const resp = await fetch('/login', {
        method: 'POST',
        credentials: 'same-origin',
        headers: { 'Content-Type': 'application/json' },
        body: JSON.stringify({ username, password }),
      });
      if (!resp.ok) {
        setError('Invalid credentials');
        return;
      }
      onLogin();
    } catch (err) {
      setError(err.message);
    } finally {
      setLoading(false);
    }
  };

  return html`
    <div class="login-container">
      <h1>ODCHub Admin</h1>
      ${error && html`<p style="color: var(--pico-del-color)">${error}</p>`}
      <form onSubmit=${submit}>
        <label>Username
          <input type="text" value=${username} onInput=${e => setUsername(e.target.value)}
                 placeholder="admin" required autofocus />
        </label>
        <label>Password
          <input type="password" value=${password} onInput=${e => setPassword(e.target.value)}
                 placeholder="password" required />
        </label>
        <button type="submit" aria-busy=${loading} disabled=${loading}>
          ${loading ? 'Signing in...' : 'Sign In'}
        </button>
      </form>
    </div>
  `;
}

// ---- Dashboard Page ----

function DashboardPage() {
  const [info, setInfo] = useState(null);

  useEffect(() => {
    api('/hub/info').then(setInfo).catch(() => {});
    const iv = setInterval(() => api('/hub/info').then(setInfo).catch(() => {}), 10000);
    return () => clearInterval(iv);
  }, []);

  if (!info) return html`<p aria-busy="true">Loading...</p>`;

  return html`
    <h2>Dashboard</h2>
    <div class="stat-grid">
      <div class="stat-card">
        <div class="stat-value">
          <span class="status-badge ${info.connected ? 'connected' : 'disconnected'}">
            ${info.connected ? 'Connected' : 'Disconnected'}
          </span>
        </div>
        <div class="stat-label">Hub Status</div>
      </div>
      <div class="stat-card">
        <div class="stat-value">${info.hub_name || '\u2014'}</div>
        <div class="stat-label">Hub Name</div>
      </div>
      <div class="stat-card">
        <div class="stat-value" style="font-size: 1.2rem">${info.topic || '\u2014'}</div>
        <div class="stat-label">Topic</div>
      </div>
      <div class="stat-card">
        <div class="stat-value">${info.user_count || 0}</div>
        <div class="stat-label">Users Online</div>
      </div>
      <div class="stat-card">
        <div class="stat-value">${info.op_count || 0}</div>
        <div class="stat-label">Operators</div>
      </div>
      <div class="stat-card">
        <div class="stat-value">${formatSize(info.total_share)}</div>
        <div class="stat-label">Total Share</div>
      </div>
    </div>
  `;
}

// ---- Users Page ----

function UsersPage({ showToast }) {
  const [users, setUsers] = useState([]);
  const [loading, setLoading] = useState(true);

  const loadUsers = useCallback(() => {
    api('/users?limit=500').then(data => {
      setUsers(data.users || []);
      setLoading(false);
    }).catch(() => setLoading(false));
  }, []);

  useEffect(() => {
    loadUsers();
    const iv = setInterval(loadUsers, 15000);
    return () => clearInterval(iv);
  }, [loadUsers]);

  const doAction = async (nick, action) => {
    const label = action === 'kick' ? 'Kicked' : action === 'ban' ? 'Banned' : action === 'gag' ? 'Gagged' : action;
    try {
      const method = (action === 'unban' || action === 'ungag') ? 'DELETE' : 'POST';
      const path = action === 'unban' ? 'ban' : action === 'ungag' ? 'gag' : action;
      await api(`/users/${encodeURIComponent(nick)}/${path}`, {
        method,
        body: JSON.stringify({ reason: `Admin action via web UI` }),
      });
      showToast(`${label} ${nick}`, 'success');
      loadUsers();
    } catch (err) {
      showToast(err.message, 'error');
    }
  };

  if (loading) return html`<p aria-busy="true">Loading users...</p>`;

  return html`
    <h2>Online Users (${users.length})</h2>
    <figure>
      <table>
        <thead>
          <tr>
            <th>Nick</th>
            <th>Share</th>
            <th>Speed</th>
            <th>Status</th>
            <th>Actions</th>
          </tr>
        </thead>
        <tbody>
          ${users.map(u => html`
            <tr key=${u.nick}>
              <td><strong>${u.nick}</strong></td>
              <td class="share-size">${formatSize(u.share)}</td>
              <td>${u.speed}</td>
              <td>${u.is_op ? html`<span class="status-badge op">OP</span>` : 'User'}</td>
              <td class="actions">
                <button class="outline secondary" onClick=${() => doAction(u.nick, 'kick')}>Kick</button>
                <button class="outline secondary" onClick=${() => doAction(u.nick, 'ban')}>Ban</button>
                <button class="outline secondary" onClick=${() => doAction(u.nick, 'gag')}>Gag</button>
              </td>
            </tr>
          `)}
          ${users.length === 0 && html`<tr><td colspan="5">No users online</td></tr>`}
        </tbody>
      </table>
    </figure>
  `;
}

// ---- Chat Page ----

function ChatPage({ showToast }) {
  const [messages, setMessages] = useState([]);
  const [input, setInput] = useState('');
  const [sending, setSending] = useState(false);
  const messagesRef = useRef(null);

  const loadHistory = useCallback(() => {
    api('/chat/history?limit=200').then(data => {
      setMessages((data.history || []).reverse());
    }).catch(() => {});
  }, []);

  useEffect(() => {
    loadHistory();

    // Connect WebSocket for real-time updates
    const proto = location.protocol === 'https:' ? 'wss:' : 'ws:';
    const ws = new WebSocket(`${proto}//${location.host}/ws?filter=chat`);
    ws.onmessage = (evt) => {
      try {
        const event = JSON.parse(evt.data);
        if (event.type === 'Chat' && event.data) {
          setMessages(prev => [...prev, {
            nickname: event.data.nick,
            chat: event.data.message,
            timestamp: Math.floor(new Date(event.data.timestamp).getTime() / 1000),
          }]);
        }
      } catch (_) {}
    };
    ws.onerror = () => {};
    ws.onclose = () => {};

    return () => ws.close();
  }, [loadHistory]);

  useEffect(() => {
    if (messagesRef.current) {
      messagesRef.current.scrollTop = messagesRef.current.scrollHeight;
    }
  }, [messages]);

  const sendMessage = async (e) => {
    e.preventDefault();
    if (!input.trim()) return;
    setSending(true);
    try {
      await api('/chat/message', {
        method: 'POST',
        body: JSON.stringify({ message: input }),
      });
      setInput('');
    } catch (err) {
      showToast(err.message, 'error');
    } finally {
      setSending(false);
    }
  };

  return html`
    <h2>Chat</h2>
    <div class="chat-container">
      <div class="chat-messages" ref=${messagesRef}>
        ${messages.map((m, i) => html`
          <div class="chat-line" key=${i}>
            <span class="chat-time">${formatTime(m.timestamp)}</span>
            <span class="chat-nick">&lt;${m.nickname}&gt;</span>
            ${' '}${m.chat}
          </div>
        `)}
        ${messages.length === 0 && html`<p style="color: var(--pico-muted-color)">No messages yet</p>`}
      </div>
      <form class="chat-input" onSubmit=${sendMessage}>
        <input type="text" value=${input} onInput=${e => setInput(e.target.value)}
               placeholder="Type a message..." disabled=${sending} />
        <button type="submit" disabled=${sending || !input.trim()}>Send</button>
      </form>
    </div>
  `;
}

// ---- Commands Page ----

function CommandsPage({ showToast }) {
  const [commands, setCommands] = useState([]);
  const [args, setArgs] = useState({});
  const [outputs, setOutputs] = useState({});

  useEffect(() => {
    api('/commands').then(data => setCommands(data.commands || [])).catch(() => {});
  }, []);

  const execute = async (name) => {
    try {
      const data = await api(`/commands/${encodeURIComponent(name)}/execute`, {
        method: 'POST',
        body: JSON.stringify({ args: args[name] || '' }),
      });
      setOutputs(prev => ({ ...prev, [name]: data.status || 'Sent' }));
      showToast(`Executed ${name}`, 'success');
    } catch (err) {
      setOutputs(prev => ({ ...prev, [name]: `Error: ${err.message}` }));
      showToast(err.message, 'error');
    }
  };

  return html`
    <h2>Bot Commands</h2>
    ${commands.length === 0 && html`<p>No commands available (database may not be connected)</p>`}
    ${commands.map(cmd => html`
      <div class="command-card" key=${cmd.name}>
        <details>
          <summary>${cmd.name}${cmd.description ? ` \u2014 ${cmd.description}` : ''}</summary>
          <div style="margin-top: 0.75rem; display: flex; gap: 0.5rem; align-items: end;">
            <input type="text" placeholder="Arguments (optional)"
                   value=${args[cmd.name] || ''}
                   onInput=${e => setArgs(prev => ({ ...prev, [cmd.name]: e.target.value }))}
                   style="margin-bottom: 0" />
            <button onClick=${() => execute(cmd.name)} style="margin-bottom: 0; white-space: nowrap">Execute</button>
          </div>
          ${outputs[cmd.name] && html`<div class="command-output">${outputs[cmd.name]}</div>`}
        </details>
      </div>
    `)}
  `;
}

// ---- Webhooks Page ----

function WebhooksPage({ showToast }) {
  const [webhooks, setWebhooks] = useState([]);
  const [showForm, setShowForm] = useState(false);
  const [form, setForm] = useState({ url: '', events: '', description: '', secret: '' });

  const loadWebhooks = useCallback(() => {
    api('/webhooks').then(data => setWebhooks(data.webhooks || [])).catch(() => {});
  }, []);

  useEffect(() => { loadWebhooks(); }, [loadWebhooks]);

  const createWebhook = async (e) => {
    e.preventDefault();
    try {
      const events = form.events.trim() ? form.events.split(',').map(s => s.trim()) : [];
      await api('/webhooks', {
        method: 'POST',
        body: JSON.stringify({
          url: form.url,
          events,
          description: form.description,
          secret: form.secret || undefined,
        }),
      });
      setForm({ url: '', events: '', description: '', secret: '' });
      setShowForm(false);
      showToast('Webhook created', 'success');
      loadWebhooks();
    } catch (err) {
      showToast(err.message, 'error');
    }
  };

  const deleteWebhook = async (id) => {
    try {
      await api(`/webhooks/${id}`, { method: 'DELETE' });
      showToast('Webhook deleted', 'success');
      loadWebhooks();
    } catch (err) {
      showToast(err.message, 'error');
    }
  };

  return html`
    <h2>Webhooks</h2>
    <button onClick=${() => setShowForm(!showForm)} class="outline">
      ${showForm ? 'Cancel' : '+ New Webhook'}
    </button>

    ${showForm && html`
      <form class="webhook-form" onSubmit=${createWebhook}>
        <label>URL
          <input type="url" value=${form.url} required
                 onInput=${e => setForm(f => ({ ...f, url: e.target.value }))}
                 placeholder="https://example.com/hook" />
        </label>
        <label>Events (comma-separated, empty = all)
          <input type="text" value=${form.events}
                 onInput=${e => setForm(f => ({ ...f, events: e.target.value }))}
                 placeholder="Chat, UserJoin, UserQuit" />
        </label>
        <label>Description
          <input type="text" value=${form.description} required
                 onInput=${e => setForm(f => ({ ...f, description: e.target.value }))}
                 placeholder="My webhook" />
        </label>
        <label>Secret (optional, for HMAC signing)
          <input type="text" value=${form.secret}
                 onInput=${e => setForm(f => ({ ...f, secret: e.target.value }))}
                 placeholder="webhook-secret" />
        </label>
        <button type="submit">Create Webhook</button>
      </form>
    `}

    <figure>
      <table>
        <thead>
          <tr>
            <th>URL</th>
            <th>Events</th>
            <th>Description</th>
            <th>Enabled</th>
            <th>Actions</th>
          </tr>
        </thead>
        <tbody>
          ${webhooks.map(wh => html`
            <tr key=${wh.id}>
              <td style="max-width: 250px; overflow: hidden; text-overflow: ellipsis">${wh.url}</td>
              <td>${wh.events.length ? wh.events.join(', ') : 'All'}</td>
              <td>${wh.description}</td>
              <td>${wh.enabled ? 'Yes' : 'No'}</td>
              <td class="actions">
                <button class="outline secondary" onClick=${() => deleteWebhook(wh.id)}>Delete</button>
              </td>
            </tr>
          `)}
          ${webhooks.length === 0 && html`<tr><td colspan="5">No webhooks configured</td></tr>`}
        </tbody>
      </table>
    </figure>
  `;
}

// ---- Main App ----

function App() {
  const [route, setRoute] = useState(window.location.hash || '#/login');
  const [toast, setToast] = useState(null);

  useEffect(() => {
    const onHash = () => setRoute(window.location.hash || '#/login');
    window.addEventListener('hashchange', onHash);
    return () => window.removeEventListener('hashchange', onHash);
  }, []);

  // Check auth on initial load
  useEffect(() => {
    if (route !== '#/login') {
      api('/hub/info').catch(() => {
        window.location.hash = '#/login';
      });
    }
  }, []);

  const showToast = (message, type) => {
    setToast({ message, type });
    clearTimeout(toastTimeout);
    toastTimeout = setTimeout(() => setToast(null), 3000);
  };

  const onLogin = () => {
    window.location.hash = '#/dashboard';
  };

  const logout = async () => {
    await fetch('/logout', { method: 'POST', credentials: 'same-origin' });
    window.location.hash = '#/login';
  };

  if (route === '#/login') {
    return html`
      <${LoginPage} onLogin=${onLogin} />
      <${Toast} ...${toast} />
    `;
  }

  const navItems = [
    ['#/dashboard', 'Dashboard'],
    ['#/users', 'Users'],
    ['#/chat', 'Chat'],
    ['#/commands', 'Commands'],
    ['#/webhooks', 'Webhooks'],
  ];

  let page;
  switch (route) {
    case '#/users': page = html`<${UsersPage} showToast=${showToast} />`; break;
    case '#/chat': page = html`<${ChatPage} showToast=${showToast} />`; break;
    case '#/commands': page = html`<${CommandsPage} showToast=${showToast} />`; break;
    case '#/webhooks': page = html`<${WebhooksPage} showToast=${showToast} />`; break;
    default: page = html`<${DashboardPage} />`; break;
  }

  return html`
    <div class="admin-layout">
      <div class="sidebar">
        <h2>ODCHub</h2>
        <ul>
          ${navItems.map(([href, label]) => html`
            <li key=${href}>
              <a href=${href} class=${route === href ? 'active' : ''}>${label}</a>
            </li>
          `)}
        </ul>
        <div class="nav-footer">
          <a href="#" onClick=${(e) => { e.preventDefault(); logout(); }}
             style="font-size: 0.85rem; color: var(--pico-muted-color)">Sign Out</a>
        </div>
      </div>
      <main class="content">
        ${page}
      </main>
    </div>
    <${Toast} ...${toast} />
  `;
}

render(html`<${App} />`, document.getElementById('app'));
