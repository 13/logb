<script lang="ts">
  // Placeholder replaced in Task 13.
  import { t } from '../i18n';
  import type { Activity, CounterUnit } from './types';
  let { objectId, unit, activities, onchanged }:
    { objectId: number; unit: CounterUnit; activities: Activity[]; onchanged?: () => void } = $props();
</script>

<p class="muted">{$t('reminder.empty')}</p>
