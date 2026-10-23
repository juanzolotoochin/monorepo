/// Declares a structure-of-arrays table and its row type. The first column must
/// be `id`. Rows are kept dense: removal swaps the last row into the hole.
macro_rules! soa_table {
    (
        $(#[$meta:meta])*
        pub struct $table:ident / $row:ident {
            pub id: $id_ty:ty,
            $($(#[$field_meta:meta])* pub $field:ident: $ty:ty,)+
        }
    ) => {
        $(#[$meta])*
        #[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
        pub struct $row {
            pub id: $id_ty,
            $($(#[$field_meta])* pub $field: $ty,)+
        }

        #[derive(Clone, Debug, Default)]
        pub struct $table {
            pub id: Vec<$id_ty>,
            $($(#[$field_meta])* pub $field: Vec<$ty>,)+
        }

        impl $table {
            #[inline]
            pub fn len(&self) -> usize {
                self.id.len()
            }

            #[inline]
            pub fn is_empty(&self) -> bool {
                self.id.is_empty()
            }

            pub fn push(&mut self, row: $row) {
                self.id.push(row.id);
                $(self.$field.push(row.$field);)+
            }

            /// Removes the row at `slot`, moving the last row into it.
            pub fn swap_remove(&mut self, slot: usize) -> $row {
                $row {
                    id: self.id.swap_remove(slot),
                    $($field: self.$field.swap_remove(slot),)+
                }
            }

            pub fn row(&self, slot: usize) -> $row {
                $row {
                    id: self.id[slot],
                    $($field: self.$field[slot].clone(),)+
                }
            }
        }

        impl $crate::hash::StateHash for $table {
            fn hash_into(&self, h: &mut $crate::hash::StateHasher) {
                h.write(&self.id);
                $(h.write(&self.$field);)+
            }
        }
    };
}

pub(crate) use soa_table;
