<script setup lang="ts">
import { NConfigProvider, NLayout, NLayoutSider, NLayoutContent, NMenu, NMessageProvider, darkTheme } from 'naive-ui'
import { computed, onMounted, onBeforeUnmount, ref } from "vue"
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

/// 窄屏默认折叠 sider。1000px 是经验阈值：≥1000 时 6 列卡片舒展，
/// <1000 卡片会被挤成 5 列偏挤。窗口尺寸用户控制时再回弹。
const COLLAPSE_BELOW = 1000
const collapsed = ref(false)
let resizeObserver: ResizeObserver | null = null

onMounted(() => {
  collapsed.value = window.innerWidth < COLLAPSE_BELOW
  resizeObserver = new ResizeObserver((entries) => {
    const w = entries[0]?.contentRect.width ?? window.innerWidth
    collapsed.value = w < COLLAPSE_BELOW
  })
  resizeObserver.observe(document.documentElement)
})

onBeforeUnmount(() => resizeObserver?.disconnect())
</script>

<template>
  <n-config-provider :theme="darkTheme" :theme-overrides="supabaseThemeOverrides">
    <n-message-provider>
      <n-layout class="app-shell" has-sider position="absolute">
        <n-layout-sider
          class="app-sider"
          bordered
          :width="240"
          :collapsed-width="64"
          :collapsed="collapsed"
          show-trigger="arrow-circle"
          @update:collapsed="(v: boolean) => (collapsed = v)"
        >
          <div class="brand">
            <span class="brand-mark"></span>
            <span v-if="!collapsed" class="brand-name">同人志档案</span>
          </div>
          <n-menu
            class="app-menu"
            :value="activeKey"
            :options="menuOptions"
            :show-tooltip="false"
            :collapsed="collapsed"
            :collapsed-width="64"
            :collapsed-icon-size="20"
            @update:value="handleMenu"
          />
          <div v-if="!collapsed" class="sider-footer">
            <span class="footer-hash">v0.1.0 · 本地</span>
          </div>
        </n-layout-sider>
        <n-layout-content class="app-content">
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
  transition: width 0.2s ease;
}
.brand {
  display: flex;
  align-items: center;
  gap: var(--spacing-8);
  padding: 18px 20px 20px;
  border-bottom: 1px solid var(--surface-border);
  height: var(--page-header-h);
  box-sizing: border-box;
}
.brand-mark {
  width: 10px;
  height: 10px;
  border-radius: 9999px;
  background: var(--color-phosphor-green);
  box-shadow: 0 0 12px var(--color-phosphor-green);
  flex-shrink: 0;
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
  width: 100%;
  height: 100%;
  overflow-y: auto;
}
</style>
