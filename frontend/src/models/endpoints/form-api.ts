import type { ReactFormExtendedApi } from '@tanstack/react-form'
import type { FormValidateOrFn } from '@tanstack/form-core'
import type { EndpointForm, ModelRouteForm } from '../../models'

/**
 * The form API a child editor may consume.
 *
 * The seventh slot is the native onSubmit validator, so the alias describes
 * exactly the form the parent creates: a form whose submit is guarded by
 * `FormValidateOrFn`. A child that only reads fields and writes values fits
 * this type without narrowing the whole API.
 */
type FormApi<TFormData> = ReactFormExtendedApi<
  TFormData,
  undefined,
  undefined,
  undefined,
  undefined,
  undefined,
  FormValidateOrFn<TFormData>,
  undefined,
  undefined,
  undefined,
  undefined,
  never
>

export type EndpointFormApi = FormApi<EndpointForm>

export type ModelRouteFormApi = FormApi<ModelRouteForm>
