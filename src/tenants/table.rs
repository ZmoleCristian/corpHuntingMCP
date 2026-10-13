use crate::model::Tenant;

use crate::tenants::ashby_1;
use crate::tenants::ashby_2;
use crate::tenants::ashby_3;
use crate::tenants::ashby_4;
use crate::tenants::ashby_5;
use crate::tenants::bamboohr_1;
use crate::tenants::bamboohr_2;
use crate::tenants::greenhouse_1;
use crate::tenants::greenhouse_2;
use crate::tenants::greenhouse_3;
use crate::tenants::jobvite;
use crate::tenants::lever_1;
use crate::tenants::lever_2;
use crate::tenants::recruitee_1;
use crate::tenants::recruitee_2;
use crate::tenants::recruitee_3;
use crate::tenants::recruitee_4;
use crate::tenants::recruitee_5;
use crate::tenants::smartrecruiters_1;
use crate::tenants::smartrecruiters_2;
use crate::tenants::smartrecruiters_3;
use crate::tenants::smartrecruiters_4;
use crate::tenants::smartrecruiters_5;
use crate::tenants::smartrecruiters_6;
use crate::tenants::smartrecruiters_7;
use crate::tenants::smartrecruiters_8;
use crate::tenants::smartrecruiters_9;
use crate::tenants::teamtailor_1;
use crate::tenants::teamtailor_2;
use crate::tenants::teamtailor_3;
use crate::tenants::teamtailor_4;
use crate::tenants::workable;
use crate::tenants::workday;

pub const TABLES: [&[Tenant]; 33] = [
    &ashby_1::TENANTS,
    &ashby_2::TENANTS,
    &ashby_3::TENANTS,
    &ashby_4::TENANTS,
    &ashby_5::TENANTS,
    &bamboohr_1::TENANTS,
    &bamboohr_2::TENANTS,
    &greenhouse_1::TENANTS,
    &greenhouse_2::TENANTS,
    &greenhouse_3::TENANTS,
    &jobvite::TENANTS,
    &lever_1::TENANTS,
    &lever_2::TENANTS,
    &recruitee_1::TENANTS,
    &recruitee_2::TENANTS,
    &recruitee_3::TENANTS,
    &recruitee_4::TENANTS,
    &recruitee_5::TENANTS,
    &smartrecruiters_1::TENANTS,
    &smartrecruiters_2::TENANTS,
    &smartrecruiters_3::TENANTS,
    &smartrecruiters_4::TENANTS,
    &smartrecruiters_5::TENANTS,
    &smartrecruiters_6::TENANTS,
    &smartrecruiters_7::TENANTS,
    &smartrecruiters_8::TENANTS,
    &smartrecruiters_9::TENANTS,
    &teamtailor_1::TENANTS,
    &teamtailor_2::TENANTS,
    &teamtailor_3::TENANTS,
    &teamtailor_4::TENANTS,
    &workable::TENANTS,
    &workday::TENANTS,
];
