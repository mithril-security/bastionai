use std::sync::Arc;

use bastionlab_common::{array_store::ArrayStore, common_conversions::to_status_error};
use linfa::{
    prelude::{SingleTargetRegression, ToConfusionMatrix},
    traits::{Fit, Predict},
    DatasetBase,
};
use ndarray::{Array2, ArrayBase, Ix1, Ix2, OwnedRepr, ViewRepr};

use tonic::Status;

use crate::{
    algorithms::*,
    trainers::{Models, PredictionTypes, SupportedModels},
    utils::{get_datasets, IArrayStore, LabelU64},
};

/// This macro converts convert the Dynamic Array Implememtation into
/// a fixed dimension say `Ix2`.
///
/// It does this by first matching on the right enum variant (considering the type
///  of the array).
///
/// It calls `into_dimensionality` to pass the dimension as a type to the macro.
macro_rules! get_inner_array {
    ($variant:tt, $array:ident, $dim:ty, $dim_str:tt, $model_name:tt, $inner:tt) => {{
        use crate::utils::{dimensionality_error, failed_array_type, IArrayStore};
        match $array {
            IArrayStore(ArrayStore::$variant(a)) => {
                let a = a
                    .into_dimensionality::<$dim>()
                    .map_err(|e| dimensionality_error(&format!("{:?}", $dim_str), e))?;
                a
            }
            _ => {
                return failed_array_type(
                    &format!("{:?} -> {:?}", $model_name, $inner),
                    ($array.0.height(), $array.0.width()),
                )
            }
        }
    }};
}

/// This macro converts `DatasetBase<IArrayBase>` into `DatasetBase<ArrayBase<T, Ix...>>`
///
macro_rules! prepare_train_data {
    ($model:tt, $train:ident, ($t_variant:tt, $t_dim:ty)) => {{
        let records = $train.records;
        let targets = $train.targets;
        let records = get_inner_array! {AxdynF64, records, Ix2, "Ix2", $model, "Records"};

        /*
            Intuitively, we ought to convert targets directly into a Ix1 but Polars' `DataFrame -> ndarray`
            conversion only uses `Array2`.

            We will have to first convert it from `DynImpl` into `Ix2` then later reshape into `Ix1`.

            Also, for the edge case of using `KMeans`, we will only choose the first column if there are multiple
            columns in the target array.
         */
        let targets = get_inner_array! {$t_variant, targets, Ix2, "Ix2", $model, "Targets"};

        // Select the first column
        let targets = targets.column(0).to_owned();

        // Here, we construct the specific DatasetBase with the right types
        DatasetBase::new(records, targets)
    }};
}
#[allow(unused)]
/// This method sends both the training and target datasets to the specified model in [`Models`].
pub fn send_to_trainer(
    records: ArrayStore,
    targets: ArrayStore,
    model_type: Models,
) -> Result<SupportedModels, Status> {
    let train = get_datasets(records, targets);

    match model_type {
        Models::GaussianNaiveBayes { var_smoothing } => {
            let train = prepare_train_data! {
                "GaussianNaiveBayes", train, (AxdynU64, Ix1)
            };

            let train = train.map_targets(|t| LabelU64(*t));

            let model = gaussian_naive_bayes(var_smoothing.into());
            Ok(SupportedModels::GaussianNaiveBayes(to_status_error(
                model.fit(&train),
            )?))
        }
        Models::ElasticNet {
            penalty,
            l1_ratio,
            with_intercept,
            max_iterations,
            tolerance,
        } => {
            let train = prepare_train_data! {"ElasticNet", train,  (AxdynF64, Ix1) };
            let model = elastic_net(
                penalty.into(),
                l1_ratio.into(),
                with_intercept,
                max_iterations,
                tolerance.into(),
            );
            Ok(SupportedModels::ElasticNet(to_status_error(
                model.fit(&train),
            )?))
        }
        Models::KMeans {
            n_runs,
            n_clusters,
            tolerance,
            max_n_iterations,
            init_method,
            random_state,
        } => {
            let train = prepare_train_data! {"KMeans", train,  (AxdynF64, Ix2) };

            /*
               For kmeans, we will set the target to Array2's default with respect to the records lenght.
               This is because the KMeans algorithm doesn't rely on the targets.

               But in order for the algorithm to work correctly and keep a unified for `prepare_train_data`, we process all the targets same
               and then later set the targets for kmeans to defaults.
            */

            let records_shape = train.records().shape().to_vec();

            let train = train
                .with_targets::<Array2<f64>>(Array2::zeros((records_shape[0], records_shape[1])));
            let model = kmeans(
                n_runs.into(),
                n_clusters.into(),
                tolerance,
                max_n_iterations,
                init_method,
                random_state,
            );
            Ok(SupportedModels::KMeans(to_status_error(model.fit(&train))?))
        }
        Models::LinearRegression { fit_intercept } => {
            let train = prepare_train_data! {"LinearRegression", train,  (AxdynF64, Ix1) };

            let model = linear_regression(fit_intercept);

            Ok(SupportedModels::LinearRegression(to_status_error(
                model.fit(&train),
            )?))
        }
        Models::TweedieRegressor {
            fit_intercept,
            alpha,
            max_iter,
            link,
            tol,
            power,
        } => {
            let train = prepare_train_data! {"TweedieRegressor", train,  (AxdynF64, Ix1) };

            let model = tweedie_regression(fit_intercept, alpha, max_iter, link, tol, power);

            Ok(SupportedModels::TweedieRegressor(to_status_error(
                model.fit(&train),
            )?))
        }

        Models::BinomialLogisticRegression {
            alpha,
            gradient_tolerance,
            fit_intercept,
            max_iterations,
            initial_params,
        } => {
            let train =
                prepare_train_data! {"BinomialLogisticRegression", train,  (AxdynU64, Ix1) };

            let model = binomial_logistic_regression(
                alpha,
                gradient_tolerance,
                fit_intercept,
                max_iterations,
                initial_params,
            );
            Ok(SupportedModels::BinomialLogisticRegression(
                to_status_error(model.fit(&train))?,
            ))
        }
        Models::MultinomialLogisticRegression {
            alpha,
            gradient_tolerance,
            fit_intercept,
            max_iterations,
            initial_params,
            shape,
        } => {
            let train =
                prepare_train_data! {"MultinomialLogisticRegression", train,  (AxdynU64, Ix1) };

            let model = multinomial_logistic_regression(
                alpha,
                gradient_tolerance,
                fit_intercept,
                max_iterations,
                initial_params,
                shape,
            )?;
            Ok(SupportedModels::MultinomialLogisticRegression(
                to_status_error(model.fit(&train))?,
            ))
        }

        Models::DecisionTree {
            split_quality,
            max_depth,
            min_weight_split,
            min_weight_leaf,
            min_impurity_decrease,
        } => {
            let train = prepare_train_data! {"DecisionTree", train,  (AxdynU64, Ix1) };

            let train = train.map_targets(|t| LabelU64(*t));
            let model = decision_trees(
                split_quality,
                max_depth,
                min_weight_split,
                min_weight_leaf,
                min_impurity_decrease,
            );
            Ok(SupportedModels::DecisionTree(to_status_error(
                model.fit(&train),
            )?))
        }
        Models::SVM {
            c,
            eps,
            nu,
            shrinking,
            platt_params,
            kernel_params,
        } => {
            todo!()
        }
    }
}

/// This method is used to run a prediction on an already fitted model, based on the model selection type.
/// We use two different types for prediction
/// [f64] and [usize] --> [PredictionTypes::Float] and [PredictionTypes::U64] respectively.
pub fn predict(
    model: Arc<SupportedModels>,
    data: ArrayStore,
    probability: bool,
) -> Result<ArrayStore, Status> {
    let sample = IArrayStore(data);
    let sample = get_inner_array! {AxdynF64, sample, Ix2, "Ix2", "predict", "sample"};
    let prediction = match &*model {
        SupportedModels::ElasticNet(m) => Some(PredictionTypes::Float(m.predict(sample))),
        SupportedModels::GaussianNaiveBayes(m) => {
            Some(PredictionTypes::U64(m.predict(sample).map_targets(|t| t.0)))
        }
        SupportedModels::KMeans(m) => Some(PredictionTypes::U64(
            m.predict(sample).map_targets(|t| *t as u64),
        )),
        SupportedModels::LinearRegression(m) => Some(PredictionTypes::Float(m.predict(sample))),
        SupportedModels::BinomialLogisticRegression(m) => {
            if probability {
                Some(PredictionTypes::SingleProbability(
                    m.predict_probabilities(&sample),
                ))
            } else {
                Some(PredictionTypes::U64(m.predict(sample)))
            }
        }
        SupportedModels::MultinomialLogisticRegression(m) => {
            if probability {
                Some(PredictionTypes::MultiProbability(
                    m.predict_probabilities(&sample),
                ))
            } else {
                Some(PredictionTypes::U64(m.predict(sample)))
            }
        }
        SupportedModels::DecisionTree(m) => {
            Some(PredictionTypes::U64(m.predict(sample).map_targets(|t| t.0)))
        }
        _ => return Err(Status::failed_precondition("Unsupported Model")),
    };

    let prediction = match prediction {
        Some(v) => match v {
            PredictionTypes::U64(pred) => ArrayStore::AxdynU64(pred.targets.into_dyn()),
            PredictionTypes::Float(pred) => ArrayStore::AxdynF64(pred.targets.into_dyn()),
            PredictionTypes::SingleProbability(pred) => ArrayStore::AxdynF64(pred.into_dyn()),
            PredictionTypes::MultiProbability(pred) => ArrayStore::AxdynF64(pred.into_dyn()),
        },
        None => return Err(Status::aborted("Failed to predict")),
    };

    Ok(prediction)
}

fn regression_metrics(
    prediction: &ArrayBase<OwnedRepr<f64>, Ix1>,
    truth: &ArrayBase<ViewRepr<&f64>, Ix1>,
    metric: &str,
) -> Result<f64, linfa::Error> {
    match metric {
        "r2" => prediction.r2(truth),
        "max_error" => prediction.max_error(truth),
        "mean_absolute_error" => prediction.mean_absolute_error(truth),
        "explained_variance" => prediction.explained_variance(truth),
        "mean_squared_log_error" => prediction.mean_squared_log_error(truth),
        "mean_squared_error" => prediction.mean_squared_error(truth),
        "median_absolute_error" => prediction.median_absolute_error(truth),
        _ => {
            return Err(linfa::Error::Priors(format!(
                "Unsupported metric: {}",
                metric
            )))
        }
    }
}

fn classification_metrics(
    prediction: &ArrayBase<OwnedRepr<LabelU64>, Ix1>,
    truth: &ArrayBase<ViewRepr<&LabelU64>, Ix1>,
    metric: &str,
) -> Result<f32, linfa::Error> {
    let cm = prediction.confusion_matrix(truth)?;
    match metric {
        "accuracy" => Ok(cm.accuracy()),
        "f1_score" => Ok(cm.f1_score()),
        "mcc" => Ok(cm.mcc()),
        _ => {
            return Err(linfa::Error::Priors(format!(
                "Could not find metric: {}",
                metric
            )))
        }
    }
}
#[allow(unused)]
pub fn inner_cross_validate(
    model: Models,
    records: ArrayStore,
    targets: ArrayStore,
    scoring: &str,
    cv: usize,
) -> Result<ArrayStore, Status> {
    let mut train = get_datasets(records, targets);

    let result = match model {
        Models::LinearRegression { fit_intercept } => {
            let m = linear_regression(fit_intercept);
            let mut train = prepare_train_data! {"LinearRegression", train,  (AxdynF64, Ix1) };
            let arr =
                to_status_error(
                    train.cross_validate_single(cv, &vec![m][..], |pred, truth| {
                        let res = regression_metrics(pred, truth, scoring);

                        match res {
                            Ok(res) => {
                                return Ok(res);
                            }
                            Err(e) => {
                                return Err(linfa::Error::Priors(format!("{e}")));
                            }
                        }
                    }),
                )?;

            ArrayStore::AxdynF64(arr.into_dyn())
        }

        Models::BinomialLogisticRegression {
            alpha,
            gradient_tolerance,
            fit_intercept,
            max_iterations,
            initial_params,
        } => {
            let m = binomial_logistic_regression(
                alpha,
                gradient_tolerance,
                fit_intercept,
                max_iterations,
                initial_params,
            );

            let mut train = prepare_train_data! {"LosgisticRegression", train,  (AxdynU64, Ix1) };

            let mut train = train.map_targets(|t| LabelU64(*t));
            let arr = to_status_error(train.cross_validate_single(
                cv,
                &vec![m][..],
                |pred, truth| classification_metrics(pred, truth, scoring),
            ))?;

            ArrayStore::AxdynF32(arr.into_dyn())
        }

        Models::MultinomialLogisticRegression {
            alpha,
            gradient_tolerance,
            fit_intercept,
            max_iterations,
            initial_params,
            shape,
        } => {
            let m = binomial_logistic_regression(
                alpha,
                gradient_tolerance,
                fit_intercept,
                max_iterations,
                initial_params,
            );

            let mut train = prepare_train_data! {"LosgisticRegression", train,  (AxdynU64, Ix1) };

            let mut train = train.map_targets(|t| LabelU64(*t));

            let arr = to_status_error(train.cross_validate_single(
                cv,
                &vec![m][..],
                |pred, truth| classification_metrics(pred, truth, scoring),
            ))?;

            ArrayStore::AxdynF32(arr.into_dyn())
        }

        _ => {
            return Err(Status::failed_precondition(format!(
                "Unsupported Model: {:?}",
                model
            )))
        }
    };

    Ok(result)
}
