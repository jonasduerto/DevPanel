<script>
  import { Server, Database, Code, Mail, Globe } from "@lucide/svelte";

  let { name, description, port, status, id, children } = $props();

  let statusClass = $derived(
    status === "Running" ? "running" : status?.startsWith?.("Error") ? "error" : "stopped"
  );

  let statusLabel = $derived(
    status === "Running" ? "Active" : status?.startsWith?.("Error") ? "Error" : "Stopped"
  );

  /** @param {string} serviceId */
  function getServiceIcon(serviceId) {
    if (serviceId.startsWith("mysql")) return Database;
    if (serviceId.startsWith("php")) return Code;
    if (serviceId.startsWith("mailpit")) return Mail;
    if (serviceId.startsWith("nginx") || serviceId.startsWith("apache")) return Globe;
    return Server;
  }

  /** @param {string} serviceId */
  function getServiceCategoryClass(serviceId) {
    if (serviceId.startsWith("mysql")) return "category-amber";
    if (serviceId.startsWith("php")) return "category-purple";
    if (serviceId.startsWith("mailpit")) return "category-sky";
    if (serviceId.startsWith("nginx")) return "category-emerald";
    if (serviceId.startsWith("apache")) return "category-pink";
    return "category-indigo";
  }

  let ServiceIcon = $derived(getServiceIcon(id));
  let categoryClass = $derived(getServiceCategoryClass(id));
</script>

<div class="service-card {statusClass} {categoryClass}">
  <div class="icon-box">
    <ServiceIcon size={16} />
  </div>

  <div class="service-info">
    <div class="service-name-row">
      <span class="service-name">{name}</span>
      {#if port}
        <span class="port-badge">:{port}</span>
      {/if}
    </div>
    <span class="service-desc">{description}</span>
  </div>

  <div class="status-badge">
    <span class="status-dot"></span>
    <span class="status-text">{statusLabel}</span>
  </div>

  <div class="service-actions">
    {@render children?.()}
  </div>
</div>

