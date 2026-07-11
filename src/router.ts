import { createRouter, createWebHistory } from 'vue-router'
const router = createRouter({
  history: createWebHistory(),
  routes: [
    { path: '/',        name: 'library',   component: () => import('./views/LibraryView.vue') },
    { path: '/library/:id', name: 'detail', component: () => import('./views/DetailView.vue') },
    { path: '/inbox',     name: 'inbox',     component: () => import('./views/InboxView.vue') },
    { path: '/inbox/compare/:id', name: 'compare', component: () => import('./views/ConflictView.vue') },
    { path: '/recycle',   name: 'recycle',   component: () => import('./views/RecycleBinView.vue') },
    { path: '/dirty',     name: 'dirty',     component: () => import('./views/DirtyView.vue') },
    { path: '/settings',  name: 'settings',  component: () => import('./views/SettingsView.vue') }
  ]
})
export default router
