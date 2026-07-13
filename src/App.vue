<script setup lang="ts">
/// App shell：64px 图标竖栏 + 内容区。
///
/// 竖栏固定 64px、永久折叠（无展开机制），菜单 4 个图标入口，
/// 设置入口搬到底部和主题切换并排。「顶部品牌名」「底部版本号」
/// 都已删除——图标自身就是入口，文字和元数据在竖栏里是噪音。

import { computed, defineComponent, h } from "vue"
import {
  NConfigProvider,
  NLayout,
  NLayoutSider,
  NLayoutContent,
  NMenu,
  NMessageProvider,
  NIcon,
  NTooltip,
  darkTheme,
  lightTheme,
} from "naive-ui"
import { RouterView, useRoute, useRouter } from "vue-router"
import { useThemeStore } from "@/stores"
import { buildThemeOverrides } from "./styles/theme-overrides"

const route = useRoute()
const router = useRouter()
const themeStore = useThemeStore()

const activeKey = computed(() => route.name as string)

/// 图标组件工厂：传入 stroke path 数组，返回一个无 props 的组件。
/// 既可被 NMenu 通过 `icon: () => h(NIcon, null, () => svgNode)` 引用，
/// 也可被 `<component :is="...">` 模板渲染。
function makeIcon(paths: string[]) {
  return defineComponent({
    name: "SiderIcon",
    setup() {
      return () =>
        h(NIcon, { size: 22 }, () =>
          h(
            "svg",
            {
              xmlns: "http://www.w3.org/2000/svg",
              viewBox: "0 0 24 24",
              fill: "none",
              stroke: "currentColor",
              "stroke-width": 1.6,
              "aria-hidden": "true",
              class: "sider-icon",
            },
            paths.map((d) =>
              h("path", { "stroke-linecap": "round", "stroke-linejoin": "round", d }),
            ),
          ),
        )
    },
  })
}

/// NMenu 的 icon 字段接收 render 函数；调用 makeIcon() 拿到组件后，
/// 用 h() 把它包成菜单要求的渲染函数形态。
function iconRender(IconComp: ReturnType<typeof makeIcon>) {
  return () => h(IconComp)
}

const iconLibrary = makeIcon([
  "M2.25 12.75V21a.75.75 0 0 0 .75.75h18a.75.75 0 0 0 .75-.75V12.75M2.25 12.75 12 3l9.75 9.75M12 3v13.5",
  "M9 21v-6h6v6",
])
const iconInbox = makeIcon([
  "M2.25 13.5h6.75a.75.75 0 0 1 .75.75v.75a1.5 1.5 0 0 0 3 0v-.75a.75.75 0 0 1 .75-.75h6.75",
  "M2.25 13.5V6.75A2.25 2.25 0 0 1 4.5 4.5h15A2.25 2.25 0 0 1 21.75 6.75v6.75",
  "M3 16.5v3.75A2.25 2.25 0 0 0 5.25 22.5h13.5A2.25 2.25 0 0 0 21 20.25V16.5",
])
const iconRecycle = makeIcon([
  "M14.74 9.75 9 4.5l-5.74 5.25",
  "M9 4.5v14.25",
  "M19.5 14.25 14.25 19.5l-5.25-5.25",
])
const iconDirty = makeIcon([
  "M12 9v3.75m9 5.85-9.39-15.4a.75.75 0 0 0-1.314-.42L.96 18.6A.75.75 0 0 0 1.61 19.8h19.78a.75.75 0 0 0 .65-1.2Z",
  "M12 17.25h.008v.008H12v-.008Z",
])
const iconCog = makeIcon([
  "M9.594 3.94c.09-.542.56-.94 1.11-.94h2.593c.55 0 1.02.398 1.11.94l.213 1.281c.063.374.313.686.645.87.074.04.147.083.22.127.325.196.72.257 1.075.124l1.217-.456a1.125 1.125 0 0 1 1.37.49l1.296 2.247a1.125 1.125 0 0 1-.26 1.431l-1.003.827c-.293.241-.438.613-.43.992a6.759 6.759 0 0 1 0 .255c-.008.379.137.75.43.991l1.005.828c.424.35.534.955.26 1.43l-1.298 2.247a1.125 1.125 0 0 1-1.369.491l-1.217-.456c-.355-.133-.75-.072-1.076.124a6.57 6.57 0 0 1-.22.128c-.331.183-.581.495-.644.869l-.213 1.28c-.09.543-.56.941-1.11.941h-2.594c-.55 0-1.02-.398-1.11-.94l-.213-1.281c-.062-.374-.312-.686-.644-.87a6.52 6.52 0 0 1-.22-.127c-.325-.196-.72-.257-1.076-.124l-1.217.456a1.125 1.125 0 0 1-1.369-.49l-1.297-2.247a1.125 1.125 0 0 1 .26-1.431l1.004-.827c.292-.24.437-.613.43-.991a6.932 6.932 0 0 1 0-.255c.007-.38-.138-.751-.43-.992l-1.004-.827a1.125 1.125 0 0 1-.26-1.43l1.297-2.247a1.125 1.125 0 0 1 1.37-.491l1.216.456c.356.133.751.072 1.076-.124.072-.044.146-.087.22-.128.332-.183.582-.495.644-.869l.214-1.281Z",
  "M15 12a3 3 0 1 1-6 0 3 3 0 0 1 6 0Z",
])

/// Sun：当 dark 时显示，点击切到 light。
const iconSun = makeIcon([
  "M12 3v2.25m6.364.386-1.591 1.591M21 12h-2.25m-.386 6.364-1.591-1.591M12 18.75V21m-4.773-4.227-1.591 1.591M5.25 12H3m4.227-4.773L5.636 5.636M15.75 12a3.75 3.75 0 1 1-7.5 0 3.75 3.75 0 0 1 7.5 0Z",
])

/// Moon：当 light 时显示，点击切到 dark。
const iconMoon = makeIcon([
  "M21.752 15.002A9.72 9.72 0 0 1 18 15.75 9.75 9.75 0 0 1 8.25 6c0-1.33.305-2.595.832-3.748a.75.75 0 0 0-1.045-.935 10.503 10.503 0 0 0-5.4 7.474.75.75 0 0 0 .574.892A10.504 10.504 0 0 0 12 21.75a10.5 10.5 0 0 0 9.749-6.748.75.75 0 0 0-.997-1Z",
])

const menuOptions = [
  { label: "我的同人志", key: "library", icon: iconRender(iconLibrary) },
  { label: "待识别", key: "inbox", icon: iconRender(iconInbox) },
  { label: "回收站", key: "recycle", icon: iconRender(iconRecycle) },
  { label: "脏数据", key: "dirty", icon: iconRender(iconDirty) },
]

function handleMenu(key: string) {
  router.push({ name: key })
}

function goSettings() {
  router.push({ name: "settings" })
}

const themeOverrides = computed(() => buildThemeOverrides(themeStore.isDark))
const naiveTheme = computed(() => (themeStore.isDark ? darkTheme : lightTheme))
</script>

<template>
  <n-config-provider :theme="naiveTheme" :theme-overrides="themeOverrides">
    <n-message-provider>
      <n-layout class="app-shell" has-sider position="absolute">
        <n-layout-sider
          class="app-sider"
          bordered
          :width="64"
          :collapsed-width="64"
          :collapsed="true"
          :native-scrollbar="false"
          :show-trigger="false"
        >
          <div class="sider-top" aria-hidden="true">
            <span class="brand-mark"></span>
          </div>

          <n-menu
            class="app-menu"
            :value="activeKey"
            :options="menuOptions"
            :collapsed="true"
            :collapsed-width="64"
            :collapsed-icon-size="22"
            :indent="18"
            @update:value="handleMenu"
          />

          <div class="sider-bottom">
            <n-tooltip placement="right">
              <template #trigger>
                <button
                  class="sider-icon-btn"
                  :aria-label="themeStore.isDark ? '切换到浅色主题' : '切换到深色主题'"
                  @click="themeStore.toggle"
                >
                  <component :is="themeStore.isDark ? iconSun : iconMoon" />
                </button>
              </template>
              {{ themeStore.isDark ? "浅色主题" : "深色主题" }}
            </n-tooltip>

            <n-tooltip placement="right">
              <template #trigger>
                <button
                  class="sider-icon-btn"
                  aria-label="设置"
                  :class="{ 'is-active': activeKey === 'settings' }"
                  @click="goSettings"
                >
                  <component :is="iconCog" />
                </button>
              </template>
              设置
            </n-tooltip>
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
}
.app-sider :deep(.n-menu-item-content) {
  justify-content: center;
}
.app-sider :deep(.n-menu-item-content__icon) {
  margin-right: 0;
}

.sider-top {
  height: 56px;
  display: flex;
  align-items: center;
  justify-content: center;
  border-bottom: 1px solid var(--surface-border);
}
.brand-mark {
  width: 10px;
  height: 10px;
  border-radius: 9999px;
  background: var(--color-phosphor-green);
  box-shadow: 0 0 12px var(--color-phosphor-green);
  flex-shrink: 0;
}

.app-menu {
  padding: 8px 0;
}

/* 底部按钮绝对定位贴底：n-layout-sider-scroll-container 子项默认
   stack 自然流，绝对定位脱离正常流避免菜单多时被挤变形。 */
.sider-bottom {
  position: absolute;
  left: 0;
  right: 0;
  bottom: 12px;
  display: flex;
  flex-direction: column;
  align-items: center;
  gap: 4px;
  pointer-events: none;
}
.sider-bottom > * {
  pointer-events: auto;
}

.sider-icon-btn {
  width: 40px;
  height: 40px;
  display: inline-flex;
  align-items: center;
  justify-content: center;
  background: transparent;
  border: 1px solid transparent;
  border-radius: 8px;
  color: var(--color-silver-mist);
  cursor: pointer;
  transition: color 0.15s, background-color 0.15s, border-color 0.15s;
}
.sider-icon-btn:hover {
  color: var(--color-snow);
  background: var(--color-ash);
  border-color: var(--surface-border);
}
.sider-icon-btn:active {
  background: var(--color-charcoal);
}
.sider-icon-btn.is-active {
  color: var(--color-phosphor-green);
  background: var(--color-forest-depth);
}

.sider-icon {
  width: 22px;
  height: 22px;
}

.app-content {
  background: var(--surface-canvas);
  width: 100%;
  height: 100%;
  overflow-y: auto;
  transition: background-color 0.2s ease;
}
</style>
