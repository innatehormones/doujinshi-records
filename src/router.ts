import { createRouter, createWebHistory } from 'vue-router'
const router = createRouter({
  history: createWebHistory(),
  routes: [
    { path: '/',        name: 'library',   component: () => import('./views/LibraryView.vue') },
    { path: '/inbox',     name: 'inbox',     component: () => import('./views/InboxView.vue') },
    { path: '/recycle',   name: 'recycle',   component: () => import('./views/RecycleBinView.vue') },
    { path: '/settings',  name: 'settings',  component: () => import('./views/SettingsView.vue') }
  ]
})
export default router
