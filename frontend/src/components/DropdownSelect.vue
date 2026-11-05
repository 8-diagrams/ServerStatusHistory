<template>
  <div ref="root" class="relative" @keydown.esc="open = false">
    <button
      type="button"
      class="flex w-full items-center justify-between gap-2 rounded-xl px-3 py-2 text-sm font-medium transition-colors focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-[#7455e9]/35"
      :class="triggerClass"
      :aria-label="label"
      aria-haspopup="listbox"
      :aria-expanded="open"
      @click="open = !open"
      @keydown.down.prevent="open = true"
    >
      <span class="flex min-w-0 items-center gap-2">
        <slot name="prefix" />
        <span class="truncate">{{ selectedLabel }}</span>
      </span>
      <svg
        viewBox="0 0 20 20"
        fill="none"
        class="h-4 w-4 shrink-0 text-[#8f84a8] transition-transform duration-200"
        :class="open ? 'rotate-180' : ''"
        aria-hidden="true"
      >
        <path d="m6 8 4 4 4-4" stroke="currentColor" stroke-width="1.8" stroke-linecap="round" stroke-linejoin="round" />
      </svg>
    </button>

    <Transition name="dropdown">
      <div
        v-if="open"
        class="absolute right-0 z-[100] mt-2 max-h-80 min-w-full overflow-y-auto rounded-xl border border-[#e8e2f2] bg-white p-1.5 shadow-[0_16px_44px_rgba(67,45,115,0.16)]"
        role="listbox"
        :aria-label="label"
      >
        <button
          v-for="option in options"
          :key="option.value"
          type="button"
          role="option"
          :aria-selected="option.value === modelValue"
          class="flex w-full items-center justify-between gap-4 whitespace-nowrap rounded-lg px-3 py-2 text-left text-sm transition-colors"
          :class="option.value === modelValue ? 'bg-[#f1edff] font-semibold text-[#6544d5]' : 'text-[#4f495d] hover:bg-[#f8f6ff] hover:text-[#6544d5]'"
          @click="select(option.value)"
        >
          <span>{{ option.label }}</span>
          <svg v-if="option.value === modelValue" viewBox="0 0 20 20" fill="none" class="h-4 w-4 text-[#7455e9]" aria-hidden="true">
            <path d="m5 10 3 3 7-7" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round" />
          </svg>
        </button>
      </div>
    </Transition>
  </div>
</template>

<script setup lang="ts">
import { onClickOutside } from '@vueuse/core'

interface DropdownOption {
  value: string
  label: string
}

const props = withDefaults(defineProps<{
  modelValue: string
  options: DropdownOption[]
  label: string
  variant?: 'soft' | 'ghost'
}>(), {
  variant: 'soft',
})

const emit = defineEmits<{
  (event: 'update:modelValue', value: string): void
}>()

const root = ref<HTMLElement>()
const open = ref(false)
const selectedLabel = computed(() => props.options.find(option => option.value === props.modelValue)?.label || '')
const triggerClass = computed(() => props.variant === 'ghost'
  ? 'bg-transparent text-[#4f495d] hover:bg-[#f5f2ff] hover:text-[#6544d5]'
  : 'bg-[#f5f2ff] text-[#4f495d] shadow-[inset_0_0_0_1px_rgba(116,85,233,0.08)] hover:bg-[#eee9ff] hover:text-[#6544d5]')

onClickOutside(root, () => {
  open.value = false
})

function select(value: string) {
  emit('update:modelValue', value)
  open.value = false
}
</script>

<style scoped>
.dropdown-enter-active,
.dropdown-leave-active {
  transition: opacity 0.16s ease, transform 0.16s ease;
}

.dropdown-enter-from,
.dropdown-leave-to {
  opacity: 0;
  transform: translateY(-5px) scale(0.98);
}
</style>
