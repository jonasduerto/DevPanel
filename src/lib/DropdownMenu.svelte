<script>
  import Popup from "#lib/Popup.svelte";

  let {
    open = $bindable(false),
    label = "More actions",
    title = label,
    align = "right",
    triggerClass = "",
    onopen = () => {},
    children,
    trigger = undefined,
  } = $props();
</script>

<Popup
  bind:open
  {label}
  {title}
  {align}
  {triggerClass}
  {onopen}
  {trigger}
>
  {@render children?.()}
</Popup>
