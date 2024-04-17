extern crate alloc;

use abstract_app::objects::{AnsAsset, AssetEntry};
use alloc::collections::BTreeMap;
use core::fmt;
use cosmwasm_std::{OverflowError, OverflowOperation, StdError, StdResult, Uint128};

#[derive(thiserror::Error, Debug, PartialEq, Eq)]
pub enum AnsAssetsError {
    #[error("Duplicate denom")]
    DuplicateDenom,
}

impl From<AnsAssetsError> for StdError {
    fn from(value: AnsAssetsError) -> Self {
        Self::generic_err(format!("Creating Coins: {value}"))
    }
}

/// A collection of assets, similar to Cosmos SDK's `sdk.AnsAssets` struct.
///
/// Differently from `sdk.AnsAssets`, which is a vector of `sdk.AnsAsset`, here we
/// implement AnsAssets as a BTreeMap that maps from asset denoms to `AnsAsset`.
/// This has a number of advantages:
///
/// - assets are naturally sorted alphabetically by denom
/// - duplicate denoms are automatically removed
/// - cheaper for searching/inserting/deleting: O(log(n)) compared to O(n)
#[derive(Clone, Default, Debug, PartialEq, Eq)]
pub struct AnsAssets(BTreeMap<AssetEntry, AnsAsset>);

/// Casting a Vec<AnsAsset> to AnsAssets.
/// The Vec can be out of order, but must not contain duplicate denoms.
/// If you want to sum up duplicates, create an empty instance using `AnsAssets::default` and
/// use `AnsAssets::add` to add your assets.
impl TryFrom<Vec<AnsAsset>> for AnsAssets {
    type Error = AnsAssetsError;

    fn try_from(vec: Vec<AnsAsset>) -> Result<Self, AnsAssetsError> {
        let mut map = BTreeMap::new();
        for asset in vec {
            if asset.amount.is_zero() {
                continue;
            }

            // if the insertion returns a previous value, we have a duplicate denom
            if map.insert(asset.name.clone(), asset).is_some() {
                return Err(AnsAssetsError::DuplicateDenom);
            }
        }

        Ok(Self(map))
    }
}

impl TryFrom<&[AnsAsset]> for AnsAssets {
    type Error = AnsAssetsError;

    fn try_from(slice: &[AnsAsset]) -> Result<Self, AnsAssetsError> {
        slice.to_vec().try_into()
    }
}

impl From<AnsAsset> for AnsAssets {
    fn from(value: AnsAsset) -> Self {
        let mut assets = AnsAssets::default();
        // this can never overflow (because there are no assets in there yet), so we can unwrap
        assets.add(value).unwrap();
        assets
    }
}

impl<const N: usize> TryFrom<[AnsAsset; N]> for AnsAssets {
    type Error = AnsAssetsError;

    fn try_from(slice: [AnsAsset; N]) -> Result<Self, AnsAssetsError> {
        slice.to_vec().try_into()
    }
}

impl From<AnsAssets> for Vec<AnsAsset> {
    fn from(value: AnsAssets) -> Self {
        value.into_vec()
    }
}

impl fmt::Display for AnsAssets {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        let s = self
            .0
            .values()
            .map(|asset| asset.to_string())
            .collect::<Vec<_>>()
            .join(",");
        write!(f, "{s}")
    }
}

impl AnsAssets {
    /// Conversion to Vec<AnsAsset>, while NOT consuming the original object.
    ///
    /// This produces a vector of assets that is sorted alphabetically by denom with
    /// no duplicate denoms.
    pub fn to_vec(&self) -> Vec<AnsAsset> {
        self.0.values().cloned().collect()
    }

    /// Conversion to Vec<AnsAsset>, consuming the original object.
    ///
    /// This produces a vector of assets that is sorted alphabetically by denom with
    /// no duplicate denoms.
    pub fn into_vec(self) -> Vec<AnsAsset> {
        self.0.into_values().collect()
    }

    /// Returns the number of different denoms in this collection.
    pub fn len(&self) -> usize {
        self.0.len()
    }

    /// Returns `true` if this collection contains no assets.
    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }

    /// Returns the denoms as a vector of strings.
    /// The vector is guaranteed to not contain duplicates and sorted alphabetically.
    pub fn entries(&self) -> Vec<AssetEntry> {
        self.0.keys().cloned().collect()
    }

    /// Returns the amount of the given denom or zero if the denom is not present.
    pub fn amount_of(&self, name: impl Into<AssetEntry>) -> Uint128 {
        self.0
            .get(&name.into())
            .map(|c| c.amount)
            .unwrap_or_else(Uint128::zero)
    }

    /// Returns the amount of the given denom if and only if this collection contains only
    /// the given denom. Otherwise `None` is returned.
    ///
    /// # Examples
    ///
    /// ```rust
    /// use carrot_app::ans_assets::{AnsAssets};
    /// use abstract_app::objects::{AnsAsset};
    ///
    /// let assets: AnsAssets = [AnsAsset::new("uatom", 100u128)].try_into().unwrap();
    /// assert_eq!(assets.contains_only("uatom").unwrap().u128(), 100);
    /// assert_eq!(assets.contains_only("uluna"), None);
    /// ```
    ///
    /// ```rust
    /// use carrot_app::ans_assets::{AnsAssets};
    /// use abstract_app::objects::{AnsAsset};
    ///
    /// let assets: AnsAssets = [AnsAsset::new("uatom", 100u128), AnsAsset::new("uusd", 200u128)].try_into().unwrap();
    /// assert_eq!(assets.contains_only("uatom"), None);
    /// ```
    pub fn contains_only(&self, name: impl Into<AssetEntry>) -> Option<Uint128> {
        if self.len() == 1 {
            self.0.get(&name.into()).map(|c| c.amount)
        } else {
            None
        }
    }

    /// Adds the given asset to this `AnsAssets` instance.
    /// Errors in case of overflow.
    pub fn add(&mut self, asset: AnsAsset) -> StdResult<()> {
        if asset.amount.is_zero() {
            return Ok(());
        }

        // if the asset is not present yet, insert it, otherwise add to existing amount
        match self.0.get_mut(&asset.name) {
            None => {
                self.0.insert(asset.name.clone(), asset);
            }
            Some(existing) => {
                existing.amount = existing.amount.checked_add(asset.amount)?;
            }
        }
        Ok(())
    }

    /// Subtracts the given asset from this `AnsAssets` instance.
    /// Errors in case of overflow or if the denom is not present.
    pub fn sub(&mut self, asset: AnsAsset) -> StdResult<()> {
        match self.0.get_mut(&asset.name) {
            Some(existing) => {
                existing.amount = existing.amount.checked_sub(asset.amount)?;
                // make sure to remove zero asset
                if existing.amount.is_zero() {
                    self.0.remove(&asset.name);
                }
            }
            None => {
                // ignore zero subtraction
                if asset.amount.is_zero() {
                    return Ok(());
                }
                return Err(OverflowError::new(
                    OverflowOperation::Sub,
                    Uint128::zero(),
                    asset.amount,
                )
                .into());
            }
        }

        Ok(())
    }

    /// Returns an iterator over the assets.
    ///
    /// # Examples
    ///
    /// ```
    /// # use cosmwasm_std::Uint128;
    /// use abstract_app::objects::AnsAsset;
    /// use carrot_app::ans_assets::AnsAssets;
    /// let mut assets = AnsAssets::default();
    /// assets.add(AnsAsset::new("uluna", 500u128)).unwrap();
    /// assets.add(AnsAsset::new("uatom", 1000u128)).unwrap();
    /// let mut iterator = assets.iter();
    ///
    /// let uatom = iterator.next().unwrap();
    /// assert_eq!(uatom.name.to_string(), "uatom");
    /// assert_eq!(uatom.amount.u128(), 1000);
    ///
    /// let uluna = iterator.next().unwrap();
    /// assert_eq!(uluna.name.to_string(), "uluna");
    /// assert_eq!(uluna.amount.u128(), 500);
    ///
    /// assert_eq!(iterator.next(), None);
    /// ```
    pub fn iter(&self) -> AnsAssetsIter<'_> {
        AnsAssetsIter(self.0.iter())
    }

    /// This is added by Abstract to extend the object with an iterator of assets
    pub fn extend(&mut self, other: impl IntoIterator<Item = AnsAsset>) -> StdResult<()> {
        other.into_iter().try_for_each(|a| self.add(a))?;

        Ok(())
    }
}

impl IntoIterator for AnsAssets {
    type Item = AnsAsset;
    type IntoIter = AnsAssetsIntoIter;

    fn into_iter(self) -> Self::IntoIter {
        AnsAssetsIntoIter(self.0.into_iter())
    }
}

impl<'a> IntoIterator for &'a AnsAssets {
    type Item = &'a AnsAsset;
    type IntoIter = AnsAssetsIter<'a>;

    fn into_iter(self) -> Self::IntoIter {
        self.iter()
    }
}

#[derive(Debug)]
pub struct AnsAssetsIntoIter(alloc::collections::btree_map::IntoIter<AssetEntry, AnsAsset>);

impl Iterator for AnsAssetsIntoIter {
    type Item = AnsAsset;

    fn next(&mut self) -> Option<Self::Item> {
        self.0.next().map(|(_, asset)| asset)
    }

    fn size_hint(&self) -> (usize, Option<usize>) {
        // Since btree_map::IntoIter implements ExactSizeIterator, this is guaranteed to return the exact length
        self.0.size_hint()
    }
}

impl DoubleEndedIterator for AnsAssetsIntoIter {
    fn next_back(&mut self) -> Option<Self::Item> {
        self.0.next_back().map(|(_, asset)| asset)
    }
}

impl ExactSizeIterator for AnsAssetsIntoIter {
    fn len(&self) -> usize {
        self.0.len()
    }
}

#[derive(Debug)]
pub struct AnsAssetsIter<'a>(alloc::collections::btree_map::Iter<'a, AssetEntry, AnsAsset>);

impl<'a> Iterator for AnsAssetsIter<'a> {
    type Item = &'a AnsAsset;

    fn next(&mut self) -> Option<Self::Item> {
        self.0.next().map(|(_, asset)| asset)
    }

    fn size_hint(&self) -> (usize, Option<usize>) {
        // Since btree_map::Iter implements ExactSizeIterator, this is guaranteed to return the exact length
        self.0.size_hint()
    }
}

impl<'a> DoubleEndedIterator for AnsAssetsIter<'a> {
    fn next_back(&mut self) -> Option<Self::Item> {
        self.0.next_back().map(|(_, asset)| asset)
    }
}

impl<'a> ExactSizeIterator for AnsAssetsIter<'a> {
    fn len(&self) -> usize {
        self.0.len()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Sort a Vec<AnsAsset> by denom alphabetically
    fn sort_by_denom(vec: &mut [AnsAsset]) {
        vec.sort_by(|a, b| a.name.cmp(&b.name));
    }

    /// Returns a mockup Vec<AnsAsset>. In this example, the assets are not in order
    fn mock_vec() -> Vec<AnsAsset> {
        vec![
            AnsAsset::new("uatom", 12345u128),
            AnsAsset::new("ibc/1234ABCD", 69420u128),
            AnsAsset::new("factory/osmo1234abcd/subdenom", 88888u128),
        ]
    }

    /// Return a mockup AnsAssets that contains the same assets as in `mock_vec`
    fn mock_assets() -> AnsAssets {
        let mut assets = AnsAssets::default();
        for asset in mock_vec() {
            assets.add(asset).unwrap();
        }
        assets
    }

    #[test]
    fn converting_vec() {
        let mut vec = mock_vec();
        let assets = mock_assets();

        // &[AnsAsset] --> AnsAssets
        assert_eq!(AnsAssets::try_from(vec.as_slice()).unwrap(), assets);
        // Vec<AnsAsset> --> AnsAssets
        assert_eq!(AnsAssets::try_from(vec.clone()).unwrap(), assets);

        sort_by_denom(&mut vec);

        // &AnsAssets --> Vec<AnsAssets>
        // NOTE: the returned vec should be sorted
        assert_eq!(assets.to_vec(), vec);
        // AnsAssets --> Vec<AnsAssets>
        // NOTE: the returned vec should be sorted
        assert_eq!(assets.into_vec(), vec);
    }

    #[test]
    fn handling_duplicates() {
        // create a Vec<AnsAsset> that contains duplicate denoms
        let mut vec = mock_vec();
        vec.push(AnsAsset::new("uatom", 67890u128));

        let err = AnsAssets::try_from(vec).unwrap_err();
        assert_eq!(err, AnsAssetsError::DuplicateDenom);
    }

    #[test]
    fn handling_zero_amount() {
        // create a Vec<AnsAsset> that contains zero amounts
        let mut vec = mock_vec();
        vec[0].amount = Uint128::zero();

        let assets = AnsAssets::try_from(vec).unwrap();
        assert_eq!(assets.len(), 2);
        assert_ne!(assets.amount_of("ibc/1234ABCD"), Uint128::zero());
        assert_ne!(
            assets.amount_of("factory/osmo1234abcd/subdenom"),
            Uint128::zero()
        );

        // adding a asset with zero amount should not be added
        let mut assets = AnsAssets::default();
        assets.add(AnsAsset::new("uusd", 0u128)).unwrap();
        assert!(assets.is_empty());
    }

    #[test]
    fn length() {
        let assets = AnsAssets::default();
        assert_eq!(assets.len(), 0);
        assert!(assets.is_empty());

        let assets = mock_assets();
        assert_eq!(assets.len(), 3);
        assert!(!assets.is_empty());
    }

    #[test]
    fn add_asset() {
        let mut assets = mock_assets();

        // existing denom
        assets.add(AnsAsset::new("uatom", 12345u128)).unwrap();
        assert_eq!(assets.len(), 3);
        assert_eq!(assets.amount_of("uatom").u128(), 24690);

        // new denom
        assets.add(AnsAsset::new("uusd", 123u128)).unwrap();
        assert_eq!(assets.len(), 4);

        // zero amount
        assets.add(AnsAsset::new("uusd", 0u128)).unwrap();
        assert_eq!(assets.amount_of("uusd").u128(), 123);

        // zero amount, new denom
        assets.add(AnsAsset::new("utest", 0u128)).unwrap();
        assert_eq!(assets.len(), 4);
    }

    #[test]
    fn sub_assets() {
        let mut assets: AnsAssets = AnsAsset::new("uatom", 12345u128).into();

        // sub more than available
        let err = assets.sub(AnsAsset::new("uatom", 12346u128)).unwrap_err();
        assert!(matches!(err, StdError::Overflow { .. }));

        // sub non-existent denom
        let err = assets.sub(AnsAsset::new("uusd", 12345u128)).unwrap_err();
        assert!(matches!(err, StdError::Overflow { .. }));

        // partial sub
        assets.sub(AnsAsset::new("uatom", 1u128)).unwrap();
        assert_eq!(assets.len(), 1);
        assert_eq!(assets.amount_of("uatom").u128(), 12344);

        // full sub
        assets.sub(AnsAsset::new("uatom", 12344u128)).unwrap();
        assert!(assets.is_empty());

        // sub zero, existing denom
        assets.sub(AnsAsset::new("uusd", 0u128)).unwrap();
        assert!(assets.is_empty());
        let mut assets: AnsAssets = AnsAsset::new("uatom", 12345u128).into();

        // sub zero, non-existent denom
        assets.sub(AnsAsset::new("uatom", 0u128)).unwrap();
        assert_eq!(assets.len(), 1);
        assert_eq!(assets.amount_of("uatom").u128(), 12345);
    }

    #[test]
    fn asset_to_assets() {
        // zero asset results in empty collection
        let assets: AnsAssets = AnsAsset::new("uusd", 0u128).into();
        assert!(assets.is_empty());

        // happy path
        let assets = AnsAssets::from(AnsAsset::new("uatom", 12345u128));
        assert_eq!(assets.len(), 1);
        assert_eq!(assets.amount_of("uatom").u128(), 12345);
    }

    #[test]
    fn exact_size_iterator() {
        let assets = mock_assets();
        let iter = assets.iter();
        assert_eq!(iter.len(), 3);
        assert_eq!(iter.size_hint(), (3, Some(3)));

        let iter = assets.into_iter();
        assert_eq!(iter.len(), 3);
        assert_eq!(iter.size_hint(), (3, Some(3)));
    }

    #[test]
    fn can_iterate_owned() {
        let assets = mock_assets();
        let mut moved = AnsAssets::default();
        for c in assets {
            moved.add(c).unwrap();
        }
        assert_eq!(moved.len(), 3);

        assert!(mock_assets().into_iter().eq(mock_assets().to_vec()));
    }

    #[test]
    fn can_iterate_borrowed() {
        let assets = mock_assets();
        assert!(assets
            .iter()
            .map(|c| &c.name)
            .eq(assets.to_vec().iter().map(|c| &c.name)));

        // can still use the assets afterwards
        assert_eq!(assets.amount_of("uatom").u128(), 12345);
    }
}
