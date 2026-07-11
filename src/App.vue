<script setup lang="ts">
import { NConfigProvider, NLayout, NLayoutSider, NLayoutContent, NMenu, NMessageProvider, darkTheme } from 'naive-ui'
import { computed } from "vue"
import { RouterView, useRoute, useRouter } from "vue-router"
import { supabaseThemeOverrides } from "./styles/theme-overrides"

const route = useRoute()
const router = useRouter()
const activeKey = computed(() => route.name as string)

const menuOptions = [
  { label: "我的同人志", key: "library" },
  { label: "待识别", key: "inbox" },
  { label: "回收站", key: "recycle" },
  { label: "脏数据", key: "dirty" },
  { label: "设置", key: "settings" }
]

function handleMenu(key: string) {
  router.push({ name: key });
}
</script>

<template>
  <n-config-provider :theme="darkTheme" :theme-overrides="supabaseThemeOverrides">
    <n-message-provider>
      <n-layout class="app-shell" has-sider>
        <n-layout-sider class="app-sider" bordered :width="240" :collapsed-width="64" show-trigger="arrow-circle">
          <div class="brand">
            <span class="brand-mark"></span>
            <span class="brand-name">同人志档案</span>
          </div>
          <n-menu
            class="app-menu"
            :value="activeKey"
            :options="menuOptions"
            :show-tooltip="false"
            @update:value="handleMenu"
          />
          <div class="sider-footer">
            <span class="footer-hash">v0.1.0 · 本地</span>
          </div>
        </n-layout-sider>
        <n-layout-content class="app-content" content-style="padding: 32px 40px">
          <router-view />
        </n-layout-content>
      </n-layout>
    </n-message-provider>
  </n-config-provider>
</template>

<style scoped>
.app-shell {
  background: var(--surface-canvas);
}
.app-sider {
  background: var(--surface-canvas) !important;
  border-right: 1px solid var(--surface-border) !important;
}
.brand {
  display: flex;
  align-items: center;
  gap: var(--spacing-8);
  padding: 20px 20px 24px;
  border-bottom: 1px solid var(--surface-border);
}
.brand-mark {
  width: 10px;
  height: 10px;
  border-radius: 9999px;
  background: var(--color-phosphor-green);
  box-shadow: 0 0 12px var(--color-phosphor-green);
}
.brand-name {
  color: var(--color-snow);
  font-size: var(--text-body-sm);
  font-weight: var(--font-weight-medium);
  letter-spacing: var(--tracking-body);
}
.app-menu {
  padding: var(--spacing-8) var(--spacing-8);
  background: transparent !important;
}
.sider-footer {
  position: absolute;
  bottom: 16px;
  left: 20px;
  right: 20px;
  border-top: 1px solid var(--surface-border);
  padding-top: 12px;
}
.footer-hash {
  font-family: var(--font-mono);
  font-size: var(--text-caption);
  color: var(--color-smoke);
  letter-spacing: 0.1em;
}
.app-content {
  background: var(--surface-canvas);
  max-width: var(--page-max-width);
  margin: 0 auto;
  width: 100%;
}
</style>
